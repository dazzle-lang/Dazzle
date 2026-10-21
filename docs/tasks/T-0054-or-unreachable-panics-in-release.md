---
id: T-0054
title: `or unreachable` panics in release
milestone: M12
status: open
kind: deferral
area: codegen
where: `dz_cpp/src/lib.rs`, `clause`
---

`or unreachable` panics in release as well as debug. §9.4 says release may assume the success path, and §18.20 has not settled whether that means `__builtin_unreachable` or merely skipping the check
