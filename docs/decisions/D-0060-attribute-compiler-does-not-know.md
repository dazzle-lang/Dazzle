---
id: D-0060
title: An attribute the compiler does not know
status: decided
---

**An error, as is a known one out of place.** Unknown names are DZ0314, a known attribute on the wrong declaration, with arguments it does not take, or repeated is DZ0315, and one the specification describes but the compiler does not implement is DZ0316. Chosen over accepting and ignoring what is not understood, the C++ and Rust-lint tradition, because an attribute changes what a declaration means — a misspelled `#[test]` silently drops a test, and a `#[packed]` that is not implemented silently gives a different layout than the one written (§12.8)
