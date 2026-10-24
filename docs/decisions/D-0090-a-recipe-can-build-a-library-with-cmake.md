---
id: D-0090
title: A recipe can build a library with its own CMake
status: decided
---

**A `[cpp.libs.<name>]` recipe may name a CMake project, `cmake = "vendor/SDL"`, instead of
listing sources. `dz` configures, builds and installs it with CMake, using its own C and C++
compilers, and takes the headers and the link line from what the build installed. Chosen by
the user on 2026-10-24, reversing r19's "no CMake" for libraries that need it (r80).**

The rule it reverses said a library with a configure step had to have that step's output
generated once and vendored. Every library until SDL was a handful of C files, so the rule
cost nothing and kept a build that is only `dz`. SDL showed the cost:
- a hand-generated `SDL_build_config.h` for each platform;
- sixty lines of source globs copied from SDL's CMakeLists, to be redone for every release;
- no macOS at all, because SDL's Cocoa backend is Objective-C `.m` files.

Most large libraries a game uses (Jolt, Assimp, Bullet, OpenAL Soft, freetype) ship a CMake
build and nothing else. A source recipe stays the way to vendor a small library.

What the form settles:

- **CMake is needed only by a recipe that uses `cmake`**, and its absence is an error naming
  that recipe. A project with no such recipe never runs it.
- **The same compilers.** `CMAKE_CXX_COMPILER` is the one `dz` compiles with, and
  `CMAKE_C_COMPILER` is the C driver of the same family (`clang` for `clang++`, `gcc` for
  `g++`). A library built by a different compiler from the one that links it is the
  classic mismatched-runtime bug.
- **Static by default.** `BUILD_SHARED_LIBS=OFF`, so the program is one executable, as
  with a source recipe. `options` can still ask for something else.
- **The build mode picks the configuration:**
  - `debug` builds CMake's `Debug`;
  - `release` and `shipping` both build `Release`.

  Each configuration is a build tree of its own under `build/cmake/<lib>/`, so switching
  modes does not rebuild the library.
- **Ninja when it is on the path**, otherwise CMake's default generator. On Windows the
  default is a Visual Studio generator, which ignores the compiler it is given, so there
  Ninja is required.
- **Built only when something changed:** the content of the files under `cmake`, the
  options, the compilers or the configuration.
- **The link line is the installed pkg-config file's.** Most CMake libraries install one
  (`lib/pkgconfig/sdl3.pc`). Its `Libs` and `Libs.private` are both linked, since the build
  is static, and its `Requires` are followed within the same install. A library that
  installs none links every library it installed, and its `include/`.
- **The editor never runs CMake.** `dz lsp` reads the headers a build installed, and until
  one has, it reports the library's headers as not built yet.

Rejected:

- **XMake, Meson or Bazel as well.** CMake is what the libraries ship. Another build system
  can be added the same way when a library needs it.
- **Calling `find_package` or a system pkg-config.** It would link whatever the machine
  happens to have rather than the vendored source.

Spec: §15.1.
