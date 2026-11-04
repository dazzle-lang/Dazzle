#!/bin/sh
# Installs dz, the Dazzle compiler, from a GitHub release (Linux and macOS):
#
#     curl -fsSL https://dazzle-lang.github.io/install.sh | sh
#
# Settings, all optional, read from the environment:
#   DAZZLE_HOME            where it goes (default ~/.dazzle; dz ends up in its bin/)
#   DAZZLE_VERSION         a release tag such as v0.1.0 (default: the latest)
#   DAZZLE_NO_MODIFY_PATH  set to 1 to leave shell startup files alone
#
# Everything is in one function called on the last line, so a download cut short runs
# nothing at all.

set -eu

REPO="https://github.com/dazzle-lang/Dazzle"

say() { printf '%s\n' "$*"; }
err() { printf 'error: %s\n' "$*" >&2; exit 1; }

download() {
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL --retry 3 -o "$2" "$1"
    elif command -v wget >/dev/null 2>&1; then
        wget -q -O "$2" "$1"
    else
        err "neither curl nor wget is installed"
    fi
}

target() {
    os=$(uname -s)
    arch=$(uname -m)
    case "$arch" in
        x86_64 | amd64) arch=x86_64 ;;
        aarch64 | arm64) arch=aarch64 ;;
        *) err "no release for $arch; build from source: https://dazzle-lang.github.io/start/install/#build-from-source" ;;
    esac
    case "$os" in
        Linux) echo "$arch-linux" ;;
        Darwin)
            # A shell under Rosetta reports x86_64 on an Apple Silicon Mac.
            if [ "$(sysctl -n hw.optional.arm64 2>/dev/null || echo 0)" = 1 ]; then
                arch=aarch64
            fi
            [ "$arch" = aarch64 ] || err "no release for Intel Macs yet; build from source: https://dazzle-lang.github.io/start/install/#build-from-source"
            echo "aarch64-macos"
            ;;
        *) err "no release for $os; on Windows use install.ps1 (see https://dazzle-lang.github.io/start/install/)" ;;
    esac
}

sha256() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d' ' -f1
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | cut -d' ' -f1
    fi
}

# The startup file of the user's shell, which is where a new PATH entry has to go.
profile() {
    case "$(basename "${SHELL:-sh}")" in
        zsh) echo "${ZDOTDIR:-$HOME}/.zshrc" ;;
        bash)
            # macOS opens every terminal as a login shell, which reads .bash_profile.
            if [ "$(uname -s)" = Darwin ]; then echo "$HOME/.bash_profile"; else echo "$HOME/.bashrc"; fi
            ;;
        fish) echo "$HOME/.config/fish/conf.d/dazzle.fish" ;;
        *) echo "$HOME/.profile" ;;
    esac
}

main() {
    home="${DAZZLE_HOME:-$HOME/.dazzle}"
    target=$(target)
    archive="dazzle-$target.tar.gz"
    if [ -n "${DAZZLE_VERSION:-}" ]; then
        base="$REPO/releases/download/$DAZZLE_VERSION"
    else
        base="$REPO/releases/latest/download"
    fi

    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT

    say "downloading $archive (${DAZZLE_VERSION:-latest release})"
    download "$base/$archive" "$tmp/$archive" || err "could not download $base/$archive"
    download "$base/SHA256SUMS" "$tmp/SHA256SUMS" || err "could not download the checksums"
    want=$(grep " $archive\$" "$tmp/SHA256SUMS" | cut -d' ' -f1)
    got=$(sha256 "$tmp/$archive")
    if [ -z "$got" ]; then
        say "warning: no sha256sum or shasum here, so the download is not verified"
    elif [ "$want" != "$got" ]; then
        err "$archive does not match its checksum; try again"
    fi

    tar -xzf "$tmp/$archive" -C "$tmp"
    # Replace what an earlier install left, but only what a release puts there.
    mkdir -p "$home"
    rm -rf "$home/bin" "$home/lib"
    for entry in "$tmp/dazzle-$target"/*; do
        mv "$entry" "$home/"
    done
    say "installed $("$home/bin/dz" --version) in $home"

    bin="$home/bin"
    case ":$PATH:" in
        *":$bin:"*) ;;
        *)
            file=$(profile)
            case "$file" in
                *.fish) line="fish_add_path \"$bin\"" ;;
                *) line="export PATH=\"$bin:\$PATH\"" ;;
            esac
            if [ "${DAZZLE_NO_MODIFY_PATH:-0}" = 1 ]; then
                say ""
                say "dz is not on your PATH. Add it with:"
                say "    $line"
            else
                mkdir -p "$(dirname "$file")"
                if ! grep -qsF "$line" "$file"; then
                    printf '\n# Dazzle\n%s\n' "$line" >>"$file"
                fi
                say "added $bin to PATH in $file; open a new terminal, or run:"
                say "    $line"
            fi
            ;;
    esac

    # dz writes C++ and hands it to a C++ compiler, which is the one thing it needs from
    # the machine. Say so now rather than at the first build.
    if ! command -v clang++ >/dev/null 2>&1 && ! command -v g++ >/dev/null 2>&1 &&
        ! command -v c++ >/dev/null 2>&1; then
        say ""
        say "dz also needs a C++17 compiler, and none was found. Install one:"
        say "    macOS:          xcode-select --install"
        say "    Debian, Ubuntu: sudo apt install g++"
        say "    Fedora:         sudo dnf install gcc-c++"
        say "    Arch:           sudo pacman -S gcc"
    elif [ "$(uname -s)" = Darwin ] &&
        [ ! -f /Library/Developer/CommandLineTools/usr/lib/libclang.dylib ] &&
        [ ! -f /Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/lib/libclang.dylib ]; then
        say ""
        say "dz reads C headers with the libclang in Xcode's command line tools. Install them:"
        say "    xcode-select --install"
    fi

    say ""
    say "Start a project:  dz new hello && cd hello && dz run"
}

main "$@"
