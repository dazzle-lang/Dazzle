---
id: D-0065
title: How `std` reaches the operating system
status: decided
---

**`extern fn name(…) -> T` with no body, allowed only in `std`**, whose body is C++ the compiler ships beside the runtime shim — not in it, so §16.4's shim still does no I/O. Not generic and not fallible: an extern function is a plain call, and `std` builds the `Error`. A program's own way to C and C++ stays a `cpp` dependency. First written as a `#[native]` attribute (r54); a keyword was chosen instead because `extern` was already reserved and is what C, C++ and Rust readers expect for "the body is elsewhere" (§7.2, §15)
