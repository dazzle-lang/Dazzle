---
id: D-0059
title: What reflection describes, and how a field carries metadata
status: decided
---

**Every kind of type, and an Odin-style tag per field.** `TypeInfo` gains `module_name`, `variants` (an enum's names and values; a union's, with each payload's fields), `elem` (a pointer's, slice's, array's or optional's element, an enum's backing type), `len`, `mutable` and `tag_offset`; a `str`, a slice and an optional expose their parts as `fields`. `FieldInfo` has `vis` and `owning` instead of a flags set, and `tag`: a raw string ending the field declaration, `name: String `json:"player_name"``, read with `tag_value(key)`. Chosen over struct-only tables, with which a serializer cannot write an enum by name or follow a slice, and over language-defined field attributes such as `#[skip]`, which a JSON or save library could not extend: a tag is text only the library reading it gives meaning to. `TypeId` is a struct around the `u32` rather than a bare `u32`, so an index is never mistaken for a number; the fields are `type_id`, `module_name` and `owning` because `type` begins a member (§8.5) and `module` and `own` are keywords. Names are Dazzle's — `List<i32>` — with the module apart, replacing §13.5's `engine.Transform`, which contradicted `::` (§13)
