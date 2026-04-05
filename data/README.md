# Build Data

This folder contains downloaded source files used by `replay-control-core/build.rs` to
generate the embedded game and arcade databases at compile time.

These files are **not checked into git** (they are large and come from upstream
repos). Only this README is tracked.

## First-time setup

After cloning the repo, run both download scripts before building:

```sh
./scripts/download-metadata.sh
./scripts/download-arcade-data.sh
```

## Contents

### Console metadata (`download-metadata.sh`)

| Directory | Source | Description |
|-----------|--------|-------------|
| `no-intro/` | [libretro/libretro-database](https://github.com/libretro/libretro-database) | No-Intro DAT files (ROM identification by CRC32) |
| `libretro-meta/genre/` | [libretro/libretro-database](https://github.com/libretro/libretro-database) | Genre classification by CRC32 |
| `libretro-meta/maxusers/` | [libretro/libretro-database](https://github.com/libretro/libretro-database) | Player count by CRC32 |
| `thegamesdb-latest.json` | [TheGamesDB](https://thegamesdb.net) | Rich metadata (year, genre, developer, players) |

### Arcade metadata (`download-arcade-data.sh`)

| File | Source | Description |
|------|--------|-------------|
| `fbneo-arcade.dat` | [libretro/FBNeo](https://github.com/libretro/FBNeo) | FBNeo ClrMame Pro XML (arcade only) |
| `mame2003plus.xml` | [libretro/mame2003-plus-libretro](https://github.com/libretro/mame2003-plus-libretro) | MAME 2003+ full XML |
| `catver.ini` | [libretro/mame2003-plus-libretro](https://github.com/libretro/mame2003-plus-libretro) | MAME 2003+ category/genre mappings |
| `mame0285-arcade.xml` | [Progetto-SNAPS](https://www.progettosnaps.net) | MAME 0.285 arcade XML |
| `catver-mame-current.ini` | [AntoPISA/MAME_SupportFiles](https://github.com/AntoPISA/MAME_SupportFiles) | Current MAME category/genre mappings |

### TGDB lookup tables (`download-tgdb-lookups.sh`)

| File | Source | Description |
|------|--------|-------------|
| `tgdb-developers.json` | [TheGamesDB API](https://api.thegamesdb.net/) | Developer ID-to-name map (~8,262 entries) |
| `tgdb-publishers.json` | [TheGamesDB API](https://api.thegamesdb.net/) | Publisher ID-to-name map (~4,452 entries) |
| `tgdb-genres.json` | [TheGamesDB API](https://api.thegamesdb.net/) | Genre ID-to-name map (~30 entries) |

These files require a (free) TGDB API key to refresh. They are committed to the repo
because the API has a 1000 requests/month limit. Only the maintainer needs the key.

```sh
TGDB_API_KEY=your_key ./scripts/download-tgdb-lookups.sh
```

## When to refresh

Re-download when upstream data changes (e.g., a genre fix gets merged):

```sh
# Re-download all metadata (including genre DATs)
./scripts/download-metadata.sh --force

# Re-download arcade data only
./scripts/download-arcade-data.sh
```

After refreshing, rebuild to bake in the updated data:

```sh
cargo build -p replay-control-core
```
