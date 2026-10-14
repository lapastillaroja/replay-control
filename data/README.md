# Build Data

This folder contains sources used to build `catalog.sqlite`. Downloaded
third-party inputs live under `upstream/` and are gitignored; curated and
monthly-refreshed snapshots under `community/`, `wikidata/`,
`retroachievements/`, and `shmups-wiki/` are tracked.

## First-time setup

After cloning the repo, run the download scripts before building:

```sh
./scripts/download-metadata.sh
./scripts/download-arcade-data.sh
./scripts/download-tgdb-lookups.sh # requires TGDB_API_KEY
```

## Contents

### Console metadata (`download-metadata.sh`)

| Directory | Source | Description |
|-----------|--------|-------------|
| `upstream/no-intro/` | [libretro/libretro-database](https://github.com/libretro/libretro-database) | No-Intro DAT files (ROM identification by CRC32) |
| `upstream/libretro-meta/genre/` | [libretro/libretro-database](https://github.com/libretro/libretro-database) | Genre classification by CRC32 |
| `upstream/libretro-meta/maxusers/` | [libretro/libretro-database](https://github.com/libretro/libretro-database) | Player count by CRC32 |
| `upstream/thegamesdb-latest.json` | [TheGamesDB](https://thegamesdb.net) | Rich metadata (year, genre, developer, players) |

### Arcade metadata (`download-arcade-data.sh`)

| File | Source | Description |
|------|--------|-------------|
| `upstream/fbneo-arcade.dat` | [libretro/FBNeo](https://github.com/libretro/FBNeo) | FBNeo ClrMame Pro XML (arcade only) |
| `upstream/mame2003plus.xml` | [libretro/mame2003-plus-libretro](https://github.com/libretro/mame2003-plus-libretro) | MAME 2003+ full XML |
| `upstream/catver.ini` | [libretro/mame2003-plus-libretro](https://github.com/libretro/mame2003-plus-libretro) | MAME 2003+ category/genre mappings |
| `upstream/mame0289-arcade.xml` | [Progetto-SNAPS](https://www.progettosnaps.net) | MAME 0.289 arcade XML |
| `upstream/catver-mame-current.ini` | [AntoPISA/MAME_SupportFiles](https://github.com/AntoPISA/MAME_SupportFiles) | Current MAME category/genre mappings |

### TGDB lookup tables (`download-tgdb-lookups.sh`)

| File | Source | Description |
|------|--------|-------------|
| `tgdb-developers.json` | [TheGamesDB API](https://api.thegamesdb.net/) | Developer ID-to-name map (~8,262 entries) |
| `tgdb-publishers.json` | [TheGamesDB API](https://api.thegamesdb.net/) | Publisher ID-to-name map (~4,452 entries) |
| `tgdb-genres.json` | [TheGamesDB API](https://api.thegamesdb.net/) | Genre ID-to-name map (~30 entries) |

These files require a (free) TGDB API key to refresh. They are downloaded into
`upstream/` and are not committed. Only the maintainer needs the key.

```sh
TGDB_API_KEY=your_key ./scripts/download-tgdb-lookups.sh
```

### Wikidata series snapshot

| File | Source | Description |
|------|--------|-------------|
| `wikidata/series.json` | [Wikidata](https://www.wikidata.org/) (CC0) | Game series, sequel, and prequel relationships used to build `catalog.sqlite` |

This snapshot is checked into git because live Wikidata SPARQL queries are public
and rate-limited. Release builds must use the committed snapshot instead of
querying Wikidata during CI. A monthly GitHub Actions workflow refreshes this
file and opens a review PR when the snapshot changes. To refresh it manually:

```sh
mkdir -p data/wikidata
python3 scripts/wikidata-series-extract.py > data/wikidata/series.json
```

### Shmups Wiki resource snapshot

| File | Source | Description |
|------|--------|-------------|
| `shmups-wiki/games.json` | [Shmups Wiki](https://shmups.wiki/) | Strategy guide and video-index link titles used to build `catalog.sqlite` resources |

This snapshot is checked into git so release builds do not depend on the wiki during CI.

### Community-curated metadata

| Directory | Description |
|-----------|-------------|
| `community/<system>.json` | Curated metadata entries for ROMs not covered by upstream sources (AmigaVision, aftermarket/homebrew releases, etc.). Committed to git; no download script. |

Anyone can open a PR adding new entries — schema and submission flow are documented in [docs/contributing/community-metadata.md](../docs/contributing/community-metadata.md).

## When to refresh

Re-download when upstream data changes (e.g., a genre fix gets merged):

```sh
# Re-download all metadata (including genre DATs)
./scripts/download-metadata.sh --force

# Re-download arcade data only
./scripts/download-arcade-data.sh

# Refresh Wikidata series data manually
python3 scripts/wikidata-series-extract.py > data/wikidata/series.json
```

After refreshing, rebuild the bundled catalog to bake in the updated data:

```sh
cargo run -p build-catalog -- --output catalog.sqlite
```
