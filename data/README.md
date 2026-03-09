# Arcade Source Data

This folder contains downloaded source files used by `replay-control-core/build.rs` to
generate the embedded arcade game database at compile time.

These files are **not checked into git** (they are large and come from upstream
repos). Only this README is tracked.

## Contents

| File                | Source | Description |
|---------------------|--------|-------------|
| `fbneo-arcade.dat`  | [libretro/FBNeo](https://github.com/libretro/FBNeo) | FBNeo ClrMame Pro XML (arcade only) |
| `mame2003plus.xml`  | [libretro/mame2003-plus-libretro](https://github.com/libretro/mame2003-plus-libretro) | MAME 2003+ full XML |
| `catver.ini`        | [libretro/mame2003-plus-libretro](https://github.com/libretro/mame2003-plus-libretro) | Category/genre mappings |

## How to populate

Run the download script from the project root:

```sh
./scripts/download-arcade-data.sh
```

This downloads the latest versions of all source files into this folder. The
script is idempotent and safe to run multiple times (it overwrites existing
files).

## When to refresh

These files change infrequently. Refresh when:

- A new RePlayOS version ships with updated emulator cores
- You want the latest game additions from FBNeo or MAME 2003+
