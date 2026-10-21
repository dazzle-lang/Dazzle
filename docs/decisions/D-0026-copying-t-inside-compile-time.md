---
id: D-0026
title: Copying a `T` inside a compile-time branch
status: decided
---

**A `copy` only where `T` is `Copy`.** Written since r13 as a bounded implementation, `impl<T: Copy> Copy for Buffer<T>`: `Buffer<f32>` copies, `Buffer<File>` is move-only, and inside the `copy` `T` is `Copy` because the implementation says so (r12 had the same effect through an implicit rule). Every copyable type satisfies `Copy`, so `fn dup<T: Copy>` can say what it needs. Bounding the container itself (`Buffer<T: Copy>`) was rejected because it would shut out every element type that owns a resource (§8.2, §8.3)
