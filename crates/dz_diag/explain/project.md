# Projects and manifests, DZ00xx

A project is a `dazzle.toml` and the sources it lists (spec §3.1). Its mistakes are in the
manifest rather than in Dazzle source, so no single-file program shows them.

## DZ0001

`dz` looks for `dazzle.toml` in the directory given, or the current one. A path that is a
file must be the manifest itself.

No example: the manifest is missing, so there is no program to show.

## DZ0002

The manifest is TOML: `key = value` lines under `[table]` headers, strings in double quotes,
arrays in brackets.

No example: the mistake is in `dazzle.toml`, not in Dazzle source.

## DZ0003

Every manifest has a `[module]` table with a `name` and the `sources` it compiles:

    [module]
    name    = "game"
    sources = ["src/**/*.dz"]

No example: the mistake is in `dazzle.toml`, not in Dazzle source.

## DZ0004

A key has the wrong type — a string where an array is wanted — or a value outside the set
the key takes, such as an `optimize` that is not `debug`, `release` or `shipping`.

No example: the mistake is in `dazzle.toml`, not in Dazzle source.

## DZ0005

A module whose `sources` match no file would compile to nothing, which is never what was
meant; usually the glob is relative to the wrong directory.

No example: the mistake is in `dazzle.toml`, not in Dazzle source.

## DZ0006

Modules may not depend on one another in a cycle (spec §3.2): each is checked after what it
depends on, which a cycle makes impossible. Move what both need into a third module.

No example: it takes two modules, each with its own `dazzle.toml`.
