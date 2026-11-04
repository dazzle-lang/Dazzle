"""Package a release of `dz`, and smoke-test the archive (T-0076).

    python tools/release.py llvm    --target x86_64-linux --out build/llvm
    python tools/release.py package --target x86_64-linux --dz target/release/dz \
                                    --llvm build/llvm --out dist
    python tools/release.py smoke   dist/dazzle-x86_64-linux.tar.gz

The release workflow (`.github/workflows/release.yml`) runs these three on each platform.

What a release holds, so that the only thing `dz` asks of the machine is a C++ compiler:

    dazzle-<target>/
      bin/dz                     (dz.exe, and libclang.dll beside it, on Windows)
      lib/libclang.so            (Linux)
      lib/clang/<major>/include  clang's own headers (stddef.h, ...), which glibc's include
      LICENSE, LICENSE-LLVM.txt

`dz` looks for libclang beside itself or in a `lib/` next to it before anywhere else, and
finds the headers by LLVM's layout rule (crates/dz_cppimport/src/clang.rs, `bundled` and
`find_resource_dir`). macOS carries no libclang: Xcode's command line tools ship one, and
they are also what gives a Mac its C++ compiler, so anyone who can build a Dazzle program
there has it already, and `dz` finds it where they put it.

libclang comes from LLVM's own release archives, which build it with LLVM linked in
statically: on Linux it needs only libc, libstdc++, libgcc and zlib. The archives are 1-2 GB,
so they are streamed and only the few files needed are kept.
"""

import argparse
import hashlib
import os
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import urllib.request
import zipfile
from pathlib import Path

# The LLVM whose libclang a release carries. Any recent one reads the headers `dz` needs;
# bump it deliberately, and the smoke test says whether the new one still does.
LLVM_VERSION = "23.1.5"
LLVM_RELEASES = f"https://github.com/llvm/llvm-project/releases/download/llvmorg-{LLVM_VERSION}"
LLVM_LICENSE = (
    f"https://raw.githubusercontent.com/llvm/llvm-project/llvmorg-{LLVM_VERSION}/clang/LICENSE.TXT"
)

# Each target, and the LLVM archive its libclang comes from (None: the system's is used).
TARGETS = {
    "x86_64-linux": f"LLVM-{LLVM_VERSION}-Linux-X64.tar.xz",
    "aarch64-linux": f"LLVM-{LLVM_VERSION}-Linux-ARM64.tar.xz",
    "aarch64-macos": None,
    "x86_64-windows": f"clang+llvm-{LLVM_VERSION}-x86_64-pc-windows-msvc.tar.xz",
}

ROOT = Path(__file__).resolve().parent.parent


def is_windows(target):
    return target.endswith("-windows")


def archive_name(target):
    return f"dazzle-{target}." + ("zip" if is_windows(target) else "tar.gz")


# ---------------------------------------------------------------------------------------
# llvm: fetch libclang and its headers


def keep(target, inner):
    """Where a member of the LLVM archive goes in the release, or None to skip it. `inner`
    is its path below the archive's top folder."""
    if is_windows(target):
        if inner == "bin/libclang.dll":
            return "bin/libclang.dll"
    elif re.fullmatch(r"lib/libclang\.so\.\d+(\.\d+)*", inner):
        # The real file; `libclang.so` and `libclang.so.23.1` are symlinks to it. `dz` looks
        # for the plain name, and loading by path does not care about the soname.
        return "lib/libclang.so"
    if re.fullmatch(r"lib/clang/\d+/include/.+", inner):
        return inner
    return None


def cmd_llvm(args):
    out = Path(args.out)
    archive = TARGETS[args.target]
    if archive is None:
        print(f"{args.target}: no libclang to bundle")
        out.mkdir(parents=True, exist_ok=True)
        return
    if out.exists():
        shutil.rmtree(out)
    out.mkdir(parents=True)
    url = f"{LLVM_RELEASES}/{archive}"
    print(f"streaming {url}")
    kept = 0
    with urllib.request.urlopen(url) as response:
        # `r|xz`: read as a stream, so the archive is never on disk whole.
        with tarfile.open(fileobj=response, mode="r|xz") as tar:
            for member in tar:
                if not member.isfile():
                    continue
                inner = member.name.split("/", 1)[-1]
                dest = keep(args.target, inner)
                if dest is None:
                    continue
                path = out / dest
                path.parent.mkdir(parents=True, exist_ok=True)
                with tar.extractfile(member) as src, open(path, "wb") as dst:
                    shutil.copyfileobj(src, dst)
                kept += 1
    lib = out / ("bin/libclang.dll" if is_windows(args.target) else "lib/libclang.so")
    if not lib.is_file():
        sys.exit(f"error: {archive} had no libclang")
    print(f"kept {kept} files")
    with urllib.request.urlopen(LLVM_LICENSE) as response:
        (out / "LICENSE-LLVM.txt").write_bytes(response.read())


# ---------------------------------------------------------------------------------------
# package: dz + libclang into one archive


def cmd_package(args):
    target = args.target
    name = f"dazzle-{target}"
    stage = Path(args.out) / name
    if stage.exists():
        shutil.rmtree(stage)
    (stage / "bin").mkdir(parents=True)
    exe = "dz.exe" if is_windows(target) else "dz"
    shutil.copy2(args.dz, stage / "bin" / exe)
    llvm = Path(args.llvm)
    if llvm.is_dir():
        shutil.copytree(llvm, stage, dirs_exist_ok=True)
    lib = stage / "lib" / "libclang.so"
    if lib.exists() and shutil.which("strip"):
        # LLVM ships it with its symbol table; dropping that is 14 MB nobody needs.
        subprocess.run(["strip", "--strip-unneeded", str(lib)], check=True)
    shutil.copy2(ROOT / "LICENSE", stage / "LICENSE")
    if not is_windows(target):
        (stage / "bin" / exe).chmod(0o755)

    out = Path(args.out) / archive_name(target)
    if is_windows(target):
        with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED, compresslevel=6) as z:
            for path in sorted(stage.rglob("*")):
                if path.is_file():
                    z.write(path, path.relative_to(stage.parent).as_posix())
    else:
        with tarfile.open(out, "w:gz", compresslevel=6) as tar:
            tar.add(stage, arcname=name)
    print(f"{out}  {out.stat().st_size / 1e6:.1f} MB")


# ---------------------------------------------------------------------------------------
# smoke: unpack an archive and build a program with it

HELLO_TOML = """[module]
name    = "hello"
sources = ["src/**/*.dz"]

[dependencies]
std = { open = true }

[build]
output = "executable"
"""

# `println!` reaches the standard library, which imports `stdlib.h` through libclang, so
# this one program proves the bundled libclang loads and finds its headers.
HELLO_DZ = """fn main() {
    let mut names = List<String>()
    names.push(String::from("Dazzle"))
    for name in names.iter() {
        println!("hello from {}", name)
    }
}
"""


def cmd_smoke(args):
    archive = Path(args.archive).resolve()
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        if archive.suffix == ".zip":
            with zipfile.ZipFile(archive) as z:
                z.extractall(tmp)
        else:
            with tarfile.open(archive) as tar:
                if hasattr(tarfile, "data_filter"):
                    tar.extractall(tmp, filter="data")
                else:
                    tar.extractall(tmp)
        (top,) = tmp.iterdir()
        dz = top / "bin" / ("dz.exe" if os.name == "nt" else "dz")
        if os.name != "nt":
            # zipfile and older tarfile drop the mode; a real install keeps it.
            dz.chmod(0o755)
        project = tmp / "hello"
        (project / "src").mkdir(parents=True)
        (project / "dazzle.toml").write_text(HELLO_TOML)
        (project / "src" / "main.dz").write_text(HELLO_DZ)

        # The machine's own libclang must not be what makes this pass.
        env = {k: v for k, v in os.environ.items() if k != "LIBCLANG_PATH"}
        run([str(dz), "--version"], env)
        result = run([str(dz), "run", str(project)], env)
        if "hello from Dazzle" not in result.stdout:
            sys.exit("error: the smoke program did not print its greeting")
    print("smoke test passed")


def run(cmd, env):
    result = subprocess.run(cmd, env=env, capture_output=True, text=True)
    print(f"$ {' '.join(cmd)}\n{result.stdout}{result.stderr}", end="")
    if result.returncode != 0:
        sys.exit(f"error: exit code {result.returncode}")
    return result


# ---------------------------------------------------------------------------------------
# sums: the checksum file the installers verify against


def cmd_sums(args):
    lines = []
    for path in sorted(Path(args.dir).iterdir()):
        if path.is_file() and path.name != "SHA256SUMS":
            digest = hashlib.sha256(path.read_bytes()).hexdigest()
            lines.append(f"{digest}  {path.name}\n")
    (Path(args.dir) / "SHA256SUMS").write_text("".join(lines))
    print("".join(lines), end="")


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    sub = parser.add_subparsers(dest="command", required=True)
    p = sub.add_parser("llvm", help="fetch libclang and clang's headers for a target")
    p.add_argument("--target", choices=TARGETS, required=True)
    p.add_argument("--out", required=True)
    p = sub.add_parser("package", help="put dz and libclang into the release archive")
    p.add_argument("--target", choices=TARGETS, required=True)
    p.add_argument("--dz", required=True)
    p.add_argument("--llvm", required=True)
    p.add_argument("--out", required=True)
    p = sub.add_parser("smoke", help="unpack an archive and build a program with it")
    p.add_argument("archive")
    p = sub.add_parser("sums", help="write SHA256SUMS for every file in a folder")
    p.add_argument("dir")
    args = parser.parse_args()
    {"llvm": cmd_llvm, "package": cmd_package, "smoke": cmd_smoke, "sums": cmd_sums}[
        args.command
    ](args)


if __name__ == "__main__":
    main()
