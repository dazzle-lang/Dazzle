---
id: D-0048
title: Compile-time conditionals, and where a location comes from
status: decided
---

**`#if FLAG { … } #else #if … { … } #else { … }` around declarations, members and statements, over registered flags only.** Built-ins `DEBUG`/`RELEASE`/`SHIPPING` and `OS_WINDOWS`/`OS_LINUX`/`OS_MACOS`; a package's own under `[flags]`; a dependency's read as `dep::FLAG` and turned on by the dependent's `flags = [...]` or `--flag`. An unknown flag is an error (DZ0309). The inactive branch is parsed but not checked. Chosen over checking both branches, which would forbid naming platform APIs absent on the host, and over free-form defines, which let a typo compile nothing. **An intrinsic default argument is the caller's**: `loc: SourceLocation = #location` records the call site, column 0 (§12.7, §12.10)
