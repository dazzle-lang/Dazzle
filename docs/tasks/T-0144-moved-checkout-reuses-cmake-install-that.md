---
id: T-0144
title: A moved checkout reuses a CMake install that names the old folder
milestone: M12
status: done
done: 2026-11-01
kind: bug
area: interop
---

**Done, 2026-11-01** (no revision; `docs/log/m12.md`). A CMake build's stamp now counts
where the build lives, so a moved or copied checkout builds its CMake libraries again, and
analysis reads an install only when it was made in this folder.

**Where the code is**

| Piece | Where |
|---|---|
| Where a build lives, and whether its install was made there | `cmake::location`, `CmakeBuild::location`, `stamp_path` and `built_here` in `dz_project/src/cmake.rs` |
| The stamp: the location in its setup line and as its third line | `build_cmake_libraries` in `crates/dz/src/native.rs` |
| An install made elsewhere is not read, so the import reports `DZ0709` | `read_cmake_installs` and the not-built check in `dz_project/src/lib.rs` |
| Tests | `a_cmake_library_is_built_again_when_its_project_moves` in `crates/dz/tests/run.rs`; `an_install_made_in_another_folder_is_reported_as_not_built` in `dz_driver/tests/cmake.rs`; `a_location_is_the_same_however_the_folder_is_named` in `dz_project/src/cmake.rs` |

**What it was.** CMake writes absolute paths into its build cache and into the install's
pkg-config file (`prefix=D:/RustProjects/Dazzle/examples/sdl3/build/...`). `dz`'s stamp
hashed the compilers, the generator, the configuration, the options and the source tree,
but not where the build lives. So after a checkout moved, the old build was taken as
current and its install read, and the header search went to a folder that no longer
existed:

```text
error[DZ0702]: cannot import `SDL3/SDL.h`
   = note: …: fatal error: 'SDL3/SDL.h' file not found
```

Nothing said the checkout had moved. A copied checkout was worse: the old folder still
existed, so the copy read the original's headers and linked its libraries without a word.
A changed source after a move would have reconfigured the tree in place, which CMake
refuses.

**Why it matters.** Moving or copying a checkout is ordinary: a new disk, a second
worktree, a renamed folder. The failure pointed at a missing header, not at its cause.

**How it was fixed.** The build's location joins the stamp's setup line, whose change wipes
the tree and builds it again, and is written as the stamp's third line for analysis to read
without knowing the compilers. The location is the build folder's canonical path, so
`examples/sdl3`, `.` and `src/..` name the same build and none of them rebuilds it. An
install whose stamp names another folder is not read, so the editor, which never runs
CMake, reports `DZ0709` ("has not been built in this folder yet") instead of "file not
found". Existing builds have the old two-line stamp and are built once more.
