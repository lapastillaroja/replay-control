# Game Metadata

How game metadata works in Replay today, what's missing, and the plan for enriching it with external sources.

**Last updated:** March 2026

---

## 1. Current State

### Embedded Databases

Replay ships two embedded metadata databases, compiled into the binary at build time via PHF maps. They are separate modules with different data models and build pipelines (see `reference/arcade-db-unification-analysis.md` for the rationale).

**game_db** (non-arcade systems, ~34K ROM entries across 20+ systems):
- Two-level model: `CanonicalGame` (shared per game) + `GameEntry` (per ROM variant)
- Fields: `display_name`, `year` (u16), `genre`, `developer`, `players`, `region`, `crc32`, `normalized_genre`
- Sources: No-Intro DATs (ROM identification), TheGamesDB JSON (metadata), libretro-database DATs (genre, players)
- Lookup: filename stem match, CRC32 fallback, normalized title fallback

**arcade_db** (all arcade systems, ~25K+ entries in a single flat map):
- Single struct: `ArcadeGameInfo`
- Fields: `rom_name`, `display_name`, `year` (&str), `manufacturer`, `players`, `rotation`, `status`, `is_clone`, `parent`, `category`, `normalized_genre`
- Sources: Flycast CSV, FBNeo DAT, MAME 2003+ XML, MAME current XML, catver.ini files
- Lookup: ROM zip stem (e.g., "mslug6")

Both databases have a `normalized_genre` field mapped to a shared taxonomy of ~18 genres at build time (Action, Adventure, Beat'em Up, Board & Card, Driving, Educational, Fighting, Maze, Music, Pinball, Platform, Puzzle, Quiz, Role-Playing, Shooter, Simulation, Sports, Strategy, Other).

### Unified GameInfo at the API Layer

Server functions return a single `GameInfo` struct regardless of whether data comes from arcade_db or game_db. The `resolve_game_info()` function in `server_fns.rs` is the only place that branches on system type. UI components work with `GameInfo` exclusively and never need to know the data source.

```rust
pub struct GameInfo {
    // Identity (always present)
    pub system: String,
    pub system_display: String,
    pub rom_filename: String,
    pub rom_path: String,
    pub display_name: String,       // always resolved, never empty

    // Common metadata
    pub year: String,               // "1991", "198?", or ""
    pub genre: String,              // normalized genre
    pub developer: String,          // manufacturer (arcade) or developer (console)
    pub players: u8,                // 0 = unknown

    // Arcade-specific (None for non-arcade)
    pub rotation: Option<String>,
    pub driver_status: Option<String>,
    pub is_clone: Option<bool>,
    pub parent_rom: Option<String>,
    pub arcade_category: Option<String>,

    // Console-specific (None for arcade)
    pub region: Option<String>,
}
```

### What's Missing

The embedded databases cover identification and basic metadata well, but lack:

- **Descriptions/synopses** -- no text summaries of games
- **Box art / cover images** -- no visual assets
- **Screenshots** -- no in-game or title screen images
- **Ratings** -- no community or critic scores
- **Publisher** -- available for arcade (via manufacturer), not tracked separately for console

These are all things that external metadata sources can provide.

---

## 2. External Metadata Sources

### Summary Comparison

| Source | Type | Auth | Bulk Download | Multi-lang | Retro Coverage | Media | Matching | Cost | License |
|--------|------|------|---------------|------------|----------------|-------|----------|------|---------|
| **ScreenScraper** | REST API v2 | Dev ID + user account | No (API only) | EN, FR, ES, DE, IT, PT+ | Excellent (218+ systems) | Box art, screenshots, video, fanart, manual, wheel, marquee | Hash (MD5/SHA1/CRC) + filename | Free | CC (media), community |
| **IGDB** | REST API v4 | Twitch OAuth2 | No | Limited | Good | Cover art, screenshots, artworks | Name search | Free (non-commercial) | Twitch TOS, no long-term cache |
| **TheGamesDB** | REST API v2 | API key | No | English only | Good | Box art, screenshots, fanart, banners | Name search | Free (very low limits) | Free, attribution |
| **MobyGames** | REST API v2 | API key (paid) | No | English only | Excellent (300K+) | Cover art, screenshots | Name search | Paid ($9.99+/mo) | Proprietary, paid API |
| **LaunchBox** | XML download | None | Yes (metadata only) | English only | Excellent (108K+) | Box art, screenshots (no download API) | Name matching | Free (metadata) | Free download, personal use |
| **RetroAchievements** | REST API v1 | API key (free) | No | English only | Very good (52+ systems) | Icons, title/in-game screenshots, box art | Hash-based | Free | Free, open API |
| **OpenVGDB** | SQLite download | None | Yes | English only | Good (cart systems) | Cover art links | Hash-based | Free | Open source |
| **Arcade Italia** | REST JSON | None | Export packs | EN, IT | Arcade only (49K+) | Screenshots, marquees, videos | MAME ROM name | Free | Free, attribution |
| **libretro-thumbnails** | Git repos | None | Yes (git clone) | N/A (images only) | Excellent | Box art, title screens, screenshots | No-Intro name matching | Free | Community, free redistribution |
| **Wikidata** | SPARQL + REST | None | Dumps available | 400+ languages | Moderate (110K+) | None (links only) | External IDs | Free (CC0) | CC0 (public domain) |
| **Hasheous** | REST API | None | No | English (proxies IGDB) | Good | Proxies IGDB | Hash-based | Free | Open source |
| **progetto-SNAPS** | Bulk downloads | None | Yes | N/A (images only) | Arcade: comprehensive | Snapshots, cabinets, marquees, flyers | MAME ROM name | Free | Free, personal use |

### Detailed Evaluation

#### ScreenScraper (screenscraper.fr) -- BEST OVERALL

- **URL:** https://screenscraper.fr/
- **API Docs:** https://screenscraper.fr/webapi2.php (API v2)
- **Authentication:** Three levels: developer ID/password, software name/version, user credentials (registered on screenscraper.fr)
- **Rate limits:** ~50K req/day total. Thread-based: free users get 1 thread; donors get up to 4+. Lower-priority users are rate-limited first under high server load
- **Multi-language:** Best in class -- EN, FR, ES, DE, IT, PT at minimum. Descriptions and genres are translated. Language selectable via API parameter
- **Systems coverage:** 218+ systems covering virtually all retro platforms (NES, SNES, N64, GB/GBC/GBA, Mega Drive, Master System, Saturn, Dreamcast, PS1/PS2/PSP, PC Engine, Neo Geo, Amiga, Amstrad CPC, Atari, DOS, MAME, FBNeo, MSX, C64, ZX Spectrum, and many more)
- **Media:** Box art (2D/3D), screenshots, title screens, videos, fanart, wheels/logos, marquees, manuals, bezels, "mix" composites
- **Matching:** Primary: ROM hash (CRC32, MD5, SHA1). Fallback: exact filename match
- **Data fields:** Title, description, genre, release date, developer, publisher, players, rating, region, language, family/series
- **Licensing:** Free, Creative Commons for media. Attribution required. Community-driven, Patreon-funded

**Verdict:** The single best source for retro game metadata. Hash-based matching is ideal since we already have CRC32 in game_db. The thread limit means bulk downloads will be slow for large libraries (a 5000-game library at 1 req/sec = ~1.5 hours), but since we cache locally this is a one-time cost.

#### libretro-thumbnails (github.com/libretro-thumbnails) -- BEST FOR IMAGES

- **URL:** https://github.com/libretro-thumbnails
- **Type:** Git repos of PNG images, one repo per system
- **Organization:** Three directories per system: `Named_Boxarts/`, `Named_Snaps/` (in-game screenshots), `Named_Titles/` (title screens)
- **Naming:** Follows No-Intro naming convention (matches our game_db filenames directly)
- **Bulk download:** `git clone` per system repo
- **Storage:** Each system repo is typically 50-300 MB. Total for all systems: ~5-10 GB. Can be selective (clone only systems the user has ROMs for)
- **Licensing:** Community-contributed, freely redistributable

**Verdict:** The best source for images because it supports bulk download and uses No-Intro naming that matches our game_db. Complements ScreenScraper (text metadata) or can be used standalone for an images-only approach.

#### LaunchBox Games Database (gamesdb.launchbox-app.com) -- BEST FOR OFFLINE TEXT

- **URL:** https://gamesdb.launchbox-app.com/
- **Type:** Downloadable XML/ZIP (https://gamesdb.launchbox-app.com/Metadata.zip). No API
- **Coverage:** 108K+ games across all major retro and modern platforms, including consoles, handhelds, computers, and arcade
- **Data fields:** Title, description (Notes), release date, developer, publisher, genre, max players, community rating, video URL, Wikipedia URL, series, cooperative support
- **Media:** Box art, screenshots, clear logos, banners -- but images are hosted on the website with no public download API
- **Matching:** Name/title-based against the XML database. No hash-based matching
- **Licensing:** Metadata XML freely downloadable. Image scraping may violate terms

**Verdict:** Excellent offline text metadata source. The downloadable XML is a major advantage over API-only sources. Can serve as the primary description/rating source with no API calls needed.

#### IGDB (igdb.com)

- **URL:** https://www.igdb.com/ | **Docs:** https://api-docs.igdb.com/
- **Auth:** Twitch OAuth2 (Client ID + Secret from dev.twitch.tv)
- **Rate limits:** 4 req/sec, 8 max concurrent
- **Multi-language:** Limited -- `game_localizations` endpoint being expanded, descriptions mainly English
- **Strengths:** Rich data (summaries, storylines, themes, age ratings, aggregated ratings), well-documented API
- **Weaknesses:** Name-only matching, caching restriction (data cannot be cached indefinitely), commercial use requires partnership
- **Licensing:** Free for non-commercial use under Twitch Developer Service Agreement

**Verdict:** Strong secondary source for descriptions and ratings, but the caching restriction and name-only matching make it unsuitable as primary for an offline-first approach.

#### Arcade Italia + progetto-SNAPS -- BEST FOR ARCADE

- **Arcade Italia URL:** http://adb.arcadeitalia.net/ | **API:** http://adb.arcadeitalia.net/service_scraper.php
- **Type:** REST JSON API (no auth required). 1 connection/IP recommended
- **Coverage:** All MAME-emulated machines (49K+). Detailed descriptions, screenshots, title screens, wheels, marquees, videos
- **Matching:** MAME ROM name (zip filename) -- matches our arcade_db directly
- **Multi-language:** EN + IT via `lang` parameter
- **Export formats:** XML, CSV, ClrMamePro DAT, HyperSpin XML, EmulationStation XML
- **progetto-SNAPS URL:** https://www.progettosnaps.net/
- **progetto-SNAPS type:** Bulk-downloadable media packs (snapshots, cabinets, marquees, flyers, icons, artworks, video snaps, manuals). Updated per MAME release

**Verdict:** The definitive source for arcade-specific metadata and media. Natural complement to our arcade_db since both use MAME ROM names as keys.

#### Other Sources

- **MobyGames:** Highest data quality (300K+ games, strong for retro/obscure) but paid API ($9.99+/mo) -- not viable for an open-source project
- **TheGamesDB:** Good media variety but rate limits are too restrictive (1K/month public, 6K lifetime private)
- **RetroAchievements:** Free hash-based matching, good for ROM identification and game icons. Metadata is limited (focused on achievements). 52+ retro systems
- **OpenVGDB:** Downloadable SQLite DB with hash-based matching for cart systems. Lightweight but limited metadata depth. Open source, used by OpenEmu
- **Wikidata:** Unmatched multi-language coverage (400+ languages, CC0). Useful as a translation source and cross-reference hub (stores IDs for IGDB, MobyGames, ScreenScraper, etc.). Not sufficient alone due to incomplete coverage and no media
- **Hasheous:** Open-source middleware for hash-to-metadata resolution. Proxies No-Intro, Redump, TOSEC, MAME DATs and IGDB. Good for batch identification but inherits IGDB's limitations
- **SteamGridDB:** Community artwork database (grids, heroes, logos, icons). Useful for high-quality artwork but not a metadata source
- **No-Intro / DAT-o-Matic:** Gold standard for console ROM identification (hash-based). Already used in our build pipeline. Not a metadata source itself
- **MAME / FBNeo DATs:** Definitive arcade ROM identification. Already used in our build pipeline. Basic metadata only (name, year, manufacturer, players, clone/parent)
- **Libretro Database:** Compiles No-Intro/Redump/MAME DATs into RetroArch's .rdb format. Already used in our build pipeline for genre/players data

---

## 3. Multi-Language Support

### Option A: English Only

| Aspect | Details |
|--------|---------|
| **Implementation** | Fetch descriptions and metadata in English only. Single field per game |
| **Storage** | Minimal -- one description, one set of names per game |
| **Sources needed** | Any source works (all support English) |
| **Effort** | Low -- straightforward single-language pipeline |
| **Consistency** | Matches current i18n approach (English-first, UI strings in English) |
| **User experience** | Good for English speakers. Non-English users see English game descriptions alongside their localized UI |

### Option B: Multi-Language

| Aspect | Details |
|--------|---------|
| **Implementation** | Fetch descriptions in multiple languages, store per-language variants |
| **Sources needed** | ScreenScraper (6+ languages for descriptions) + Wikidata (400+ languages for titles) |
| **Storage** | 3-6x more text storage per game (one description per language) |
| **Effort** | Medium-High -- multi-language schema, language selection logic, fallback chains |
| **Data model impact** | `GameInfo` needs per-locale description storage or a separate lookup by locale |
| **Integration** | Hooks into the existing i18n system to select the right language |
| **Coverage gaps** | ScreenScraper has good EN/FR/ES/DE/IT/PT coverage. Other languages would have significant gaps |
| **Language tiers** | Tier 1: Wikidata (400+ languages, labels only). Tier 2: ScreenScraper (6+ languages, full descriptions). Tier 3: IGDB (limited localizations). Tier 4: Arcade Italia (EN + IT only) |

### Comparison

| Criterion | Option A (English Only) | Option B (Multi-Language) |
|-----------|------------------------|--------------------------|
| Implementation effort | Low (~1 week) | Medium-High (~3 weeks) |
| Storage per game (text) | ~2 KB | ~8-12 KB |
| Total storage (10K games) | ~20 MB | ~80-120 MB |
| Source requirements | Any source | ScreenScraper required |
| User reach | English speakers | Broader audience |
| Maintenance | Simple | Complex (language fallback, coverage gaps) |
| Future i18n alignment | Can add later | Built-in from day one |
| Risk | Low | Medium (incomplete translations, inconsistent quality) |

### Assessment

Option A (English Only) is the pragmatic choice for the initial implementation. The current app UI is English-only, game names in the embedded databases are English, and all sources provide English data. Multi-language metadata can be added later as an enhancement without rearchitecting -- it just means storing additional description fields per game.

---

## 4. Local Storage Design

All external metadata should be stored locally on-device. The Pi runs offline or on slow networks, so network calls during normal browsing must be avoided. Metadata is fetched once (or periodically updated) and served from local storage.

### Storage Format Options

| Format | Pros | Cons | Best For |
|--------|------|------|----------|
| **SQLite DB** | Queryable, efficient for text lookups, single file, well-supported in Rust (rusqlite) | Additional dependency, more complex than flat files | Text metadata (descriptions, ratings) |
| **Flat files (JSON/TOML)** | Simple, human-readable, easy to debug | Slow for large datasets, no indexing | Small datasets, configuration |
| **Embedded binary (PHF)** | Fastest lookup, zero runtime overhead | Requires rebuild, not user-updatable | Already used for game_db/arcade_db |
| **Filesystem with naming convention** | Natural for images, easy to manage | Requires path conventions, many small files | Images (box art, screenshots) |

### Recommended: SQLite for Text + Filesystem for Images

### Storage Location Strategy

RePlayOS supports multiple storage backends — the user can choose SD card, USB drive, or NFS share for their ROM collection. Metadata and asset files need to be split across two locations:

**ROM storage** (`<rom_storage>/.replay-control/`) — follows the user's ROM collection:
- `metadata.db` — per-game text metadata (descriptions, ratings). Travels with the ROMs so metadata stays associated with the collection regardless of which storage device is active
- `media/` — per-game images (box art, screenshots). Same reasoning — if the user moves their USB drive to another Pi, the metadata comes along

**System storage** (`/var/lib/replay-control/`) — stays on the SD card:
- `sources/` — bulk reference data (LaunchBox XML, libretro-thumbnails repos, progetto-SNAPS packs). These are NOT per-game — they're lookup databases used during the processing/import step. They don't need to follow the ROM collection
- App config, credentials, RA cache

This split matters because:
- **NFS users** shouldn't have source blobs on the network share (slow reads, bandwidth waste, shared storage)
- **USB users** benefit from metadata traveling with the drive, but source blobs don't need to
- **SD-only users** — everything ends up on the same device anyway

**Text metadata** (descriptions, ratings, publisher): stored in a SQLite database at `<rom_storage>/.replay-control/metadata.db`.

Schema:

```sql
CREATE TABLE game_metadata (
    system TEXT NOT NULL,
    rom_filename TEXT NOT NULL,
    description TEXT,
    rating REAL,          -- 0.0-5.0 scale, normalized from source
    publisher TEXT,
    box_art_path TEXT,    -- relative path to image file
    screenshot_path TEXT, -- relative path to image file
    source TEXT NOT NULL, -- "screenscraper", "launchbox", etc.
    fetched_at INTEGER NOT NULL, -- Unix timestamp
    PRIMARY KEY (system, rom_filename)
);
```

**Images** (box art, screenshots): stored on ROM storage under `<rom_storage>/.replay-control/media/<system>/`.

**Source blobs** (optional): stored on the SD card under `/var/lib/replay-control/sources/`.

```
# On ROM storage (SD, USB, or NFS — follows the ROM collection)
<rom_storage>/.replay-control/
  metadata.db
  media/
    nintendo_snes/
      boxart/
        Super Mario World (USA).png
      snap/
        Super Mario World (USA).png
    arcade_mame/
      boxart/
        sf2.png
      snap/
        sf2.png

# On SD card (system storage — never on USB/NFS)
/var/lib/replay-control/
  sources/              (optional, configurable)
    launchbox/
      launchbox-metadata.xml
    libretro-thumbnails/
      Nintendo - Super Nintendo Entertainment System/
        Named_Boxarts/
        Named_Snaps/
    progetto-snaps/
      snap/
      titles/
```

Image filenames match ROM filename stems, making lookup trivial from `GameInfo.rom_filename`.

### Using /tmp for Source Blobs

An alternative to persisting source blobs on the SD card is using `/tmp` as a temporary processing area.

**How /tmp works on RePlayOS (typical Pi Linux):**
- Usually a `tmpfs` mount (RAM-backed), meaning contents are lost on reboot
- Size is typically limited to 50% of RAM. On a Pi 4 with 1-4 GB RAM, that's 512 MB - 2 GB available
- On some configurations, `/tmp` is a regular directory on the root filesystem (SD card)

**Analysis:**

| Approach | Pros | Cons |
|----------|------|------|
| **Persist on SD** (`/var/lib/`) | Survives reboots, no re-download needed, no RAM pressure | Uses SD card space (1.5-4 GB), SD wear from writes |
| **Use /tmp** (tmpfs) | Auto-cleanup, no permanent disk usage, no SD wear | Lost on reboot (must re-download), RAM constrained (~512 MB-2 GB), source blobs (1.5-4 GB) often exceed available /tmp space |
| **Use /tmp** (disk-backed) | Auto-cleanup on reboot | Effectively same SD usage as persist, but no control over cleanup timing |
| **Transient processing** (download → process → delete) | Minimal space: only one source at a time in /tmp | Slow: must re-download on every refresh, no offline re-processing |

**Recommendation: Transient processing as default, persistent on SD as opt-in.**

- **Default behavior:** Download a source blob to `/tmp`, process it (extract metadata into `metadata.db` + images into `media/`), then delete the blob. Only one source blob in `/tmp` at a time — LaunchBox XML (~300 MB) is the largest single file, which fits in most `/tmp` configurations. libretro-thumbnails repos can be processed per-system (50-300 MB each)
- **"Full" quality tier:** If the user explicitly opts for the Full tier in Metadata Management, source blobs are persisted to `/var/lib/replay-control/sources/` on the SD card for offline re-processing. This tier is only recommended for 32 GB+ cards
- **Fallback:** If `/tmp` is too small for a source blob (detected before download), fall back to a temporary directory on the SD card (`/var/lib/replay-control/tmp/`) and clean up after processing

### Storage Size Estimates

#### Processed Metadata (descriptions in SQLite, resized images)

| Content | Per Game | 10K Games | 50K Games |
|---------|----------|-----------|-----------|
| Text only (description, rating) | ~2 KB | ~20 MB | ~100 MB |
| Box art (resized to 256px wide) | ~30 KB | ~300 MB | ~1.5 GB |
| Screenshots (resized to 320px) | ~40 KB | ~400 MB | ~2 GB |
| All combined | ~72 KB | ~720 MB | ~3.6 GB |

#### Source Blobs (optional, on SD card at `/var/lib/replay-control/sources/`)

| Source | Size | Notes |
|--------|------|-------|
| LaunchBox `launchbox-metadata.xml` (from Metadata.zip) | ~250-300 MB | XML with 108K+ games |
| libretro-thumbnails (per system repo) | 50-300 MB each | Typical setup with 10-15 systems: ~1-3 GB. Only clone systems the user has ROMs for |
| progetto-SNAPS packs (screenshots + titles) | ~200-500 MB | Varies by MAME version |
| catver.ini / MAME XMLs | < 10 MB | Negligible |
| **Total source blobs** | **~1.5-4 GB** | Depends on number of systems and packs selected |

#### Combined Total (split by storage location)

| Scenario | ROM Storage (SD/USB/NFS) | SD Card (system) | Temp (/tmp during processing) |
|----------|--------------------------|-------------------|-------------------------------|
| Text metadata only (5K games) | ~10 MB | 0 | ~300 MB peak (LaunchBox XML) |
| Text + images (5K games) | ~360 MB | 0 | ~300 MB peak |
| Text + images, sources persisted (5K games, 10 systems) | ~360 MB | ~2 GB | 0 (already on disk) |
| Full (10K games, 15 systems, sources persisted) | ~720 MB | ~3.5 GB | 0 |

For a typical RePlayOS setup with 2K-5K games, expect 150-360 MB on ROM storage for metadata+images. Text-only metadata is negligible at 4-10 MB. Source blobs are transient by default (processed in `/tmp` then deleted) and only persist on SD if the user chooses the Full tier.

#### Storage Budget Analysis

On a **16 GB SD card** (minimum recommended by RePlayOS):

| Component | Size |
|-----------|------|
| OS + system | ~1.5 GB |
| ROMs (if stored on SD) | ~8-10 GB |
| **Available for metadata** | **~4-6 GB** |
| Processed metadata (text only) | ~10 MB |
| Processed metadata (text + images) | ~360 MB |
| Persisted source blobs | ~2-4 GB (would consume most headroom) |

When ROMs are on USB/NFS, the SD card has much more headroom (~14 GB free), making persistent source blobs feasible even on 16 GB cards.

**Recommendations by setup:**

- **16 GB SD, ROMs on SD:** Text metadata only, transient blob processing via `/tmp`. Keep metadata under ~50 MB
- **16 GB SD, ROMs on USB/NFS:** Text + images on ROM storage, transient blob processing. SD card has plenty of headroom
- **32 GB+ SD, any ROM location:** Full tier viable — text + images on ROM storage, persistent source blobs on SD

**Configurable in Metadata Management page:** Expose a "Metadata quality" option with three tiers:

- **Text Only** — descriptions and ratings in SQLite on ROM storage, no images, transient blob processing (~10 MB on ROM storage)
- **Text + Images** — adds resized box art and screenshots on ROM storage, transient blob processing (~360 MB on ROM storage for 5K games)
- **Full** — same as above, plus persistent source cache on SD card for offline re-processing (~2-4 GB additional on SD)

### Clear Metadata Operation

Clear operations target different storage locations. Exposed on the Metadata Management page:

```rust
/// Clear processed metadata (on ROM storage)
pub fn clear_processed_metadata(rom_storage: &Path) -> Result<()> {
    let rc_dir = rom_storage.join(".replay-control");
    let db_path = rc_dir.join("metadata.db");
    let media_dir = rc_dir.join("media");
    if db_path.exists() { fs::remove_file(&db_path)?; }
    if media_dir.exists() { fs::remove_dir_all(&media_dir)?; }
    Ok(())
}

/// Clear source blobs (on SD card system storage)
pub fn clear_source_cache() -> Result<()> {
    let sources_dir = Path::new("/var/lib/replay-control/sources");
    if sources_dir.exists() { fs::remove_dir_all(sources_dir)?; }
    Ok(())
}
```

---

## 5. Integration with GameInfo

### Extending GameInfo

External metadata adds new fields to `GameInfo`:

```rust
pub struct GameInfo {
    // ... existing fields ...

    // External metadata (from local cache, None if not yet fetched)
    pub description: Option<String>,
    pub rating: Option<f32>,         // 0.0-5.0
    pub box_art_url: Option<String>, // served via /media/<system>/boxart/<stem>.png
    pub screenshot_url: Option<String>,
}
```

These fields are `Option` because external metadata may not have been downloaded yet, or may not exist for a given game. The UI renders them conditionally with `<Show>`.

### Resolution Chain

`resolve_game_info()` extends to a three-level fallback:

1. **Embedded DB** (always available, zero cost): display name, year, genre, developer, players, rotation, status, region. This is the current behavior.
2. **Local metadata cache** (available after download): description, rating, box art path, screenshot path. Queried from SQLite.
3. **Fallback/placeholder**: if no cached metadata, fields are `None`. The UI shows the game without description or images.

```rust
fn resolve_game_info(system: &str, rom_filename: &str, rom_path: &str) -> GameInfo {
    // Step 1: Build GameInfo from embedded DB (existing code)
    let mut info = resolve_from_embedded_db(system, rom_filename, rom_path);

    // Step 2: Enrich from local metadata cache
    if let Ok(Some(cached)) = metadata_cache::lookup(system, rom_filename) {
        info.description = cached.description;
        info.rating = cached.rating;
        info.box_art_url = cached.box_art_path
            .map(|p| format!("/media/{system}/boxart/{p}"));
        info.screenshot_url = cached.screenshot_path
            .map(|p| format!("/media/{system}/snap/{p}"));
    }

    info
}
```

The embedded DB lookup is always fast (PHF map, ~nanoseconds). The SQLite lookup adds ~microseconds. No network calls happen during page rendering.

---

## 6. Download/Sync Strategy

### Initial Bulk Download

On first run or when triggered from Settings:

1. Scan all ROM directories to build a list of `(system, rom_filename)` pairs
2. For each game without cached metadata, queue a fetch from the chosen source
3. Download text metadata first (fast, small), then images (slow, large)
4. Store results in SQLite + filesystem as described in section 4

### Incremental Updates

When new ROMs are added (detected via filesystem scan):

1. Compare current ROM list against metadata DB entries
2. Fetch metadata only for ROMs that have no entry in the cache
3. Skip ROMs that already have cached metadata (unless user forces a refresh)

### Progress Tracking

Bulk download needs visible progress since it can take 30+ minutes for large libraries:

- Server-sent events (SSE) or polling endpoint for progress
- UI shows: games processed / total games, current game name, estimated time remaining
- Download can be paused/resumed (track which games are done in the DB)

### Bandwidth Considerations

The Pi may be on WiFi (2.4 GHz, shared bandwidth) or Ethernet:

- **Text only:** ~2 KB/game = 10 MB for 5K games. Trivial even on slow WiFi
- **With images:** ~70 KB/game = 350 MB for 5K games. Takes ~10 min on a 5 Mbps connection
- **Rate limiting:** Respect source API limits (ScreenScraper: 1 thread for free users)
- **Retry logic:** Network failures are expected -- retry with exponential backoff

---

## 7. Metadata Management Page (`/more/metadata`)

Due to data licensing restrictions (metadata can't be bundled in the binary, some sources require user credentials, storage varies by SD card size), metadata management needs its own dedicated page rather than a small section in Settings.

The page is accessible from the More menu and provides:

- **Status overview** — coverage stats (games with descriptions, box art, screenshots), storage breakdown, last sync time
- **Download/sync** — bulk download with progress bar, quality tier selector (Text Only / Text + Images / Full), per-system toggles, cancel button
- **Credentials** — ScreenScraper account (username + password + dev ID), RetroAchievements account (username + API key), with "Test Connection" buttons
- **Cache management** — clear all metadata, clear images only, clear source cache. Each with confirmation and space-to-be-freed estimate
- **Attribution** — visible source credits as required by data licenses

See `docs/features.md` (Metadata Management section) for the full feature breakdown and future ideas.

---

## 8. Implementation Plan

### Phase 1: Text Metadata (descriptions, ratings)

**Source:** LaunchBox XML (bulk download, no API calls, 108K+ games) + ScreenScraper API (for gaps).

**Scope:**
- Add `metadata.db` SQLite schema and basic CRUD operations
- Parse LaunchBox `launchbox-metadata.xml` and populate the local DB for matching games
- Extend `GameInfo` with `description` and `rating` fields
- Extend `resolve_game_info()` to query the metadata cache
- Add description display to the game detail page
- Add "Download Metadata" and "Clear Cache" to Settings

**Effort:** ~2-3 weeks.

### Phase 2: Images (box art, screenshots)

**Source:** libretro-thumbnails (git clone per system, No-Intro naming matches game_db) + progetto-SNAPS (for arcade).

**Scope:**
- Download/clone libretro-thumbnails repos for systems the user has
- Download progetto-SNAPS packs for arcade systems
- Map thumbnail filenames to ROM filenames (handle naming differences)
- Serve images via the existing Axum static file handler
- Add box art display to game list and detail pages
- Add screenshot display to detail page
- Progress tracking for image downloads

**Effort:** ~2-3 weeks.

### Phase 3: Multi-Language Support (optional)

**Source:** ScreenScraper (descriptions in 6+ languages) + Wikidata (title translations).

**Scope:**
- Extend metadata DB schema for per-language descriptions
- Add language preference to Settings
- Integrate with i18n system for language selection
- Implement language fallback chain (user language -> English -> any available)

**Effort:** ~2-3 weeks.

---

## 9. Recommendations

### Metadata Sources

**Primary for text metadata:** LaunchBox XML. It provides descriptions, ratings, genres, and player counts for 108K+ games in a single downloadable ZIP. No API calls, no rate limits, no authentication. Parse once and populate the local SQLite DB.

**Primary for images:** libretro-thumbnails (console/handheld) + progetto-SNAPS (arcade). Git repos / bulk packs of images that can be downloaded without API calls. libretro-thumbnails follows No-Intro naming (matches game_db), progetto-SNAPS uses MAME ROM names (matches arcade_db).

**Secondary/fallback:** ScreenScraper API. When LaunchBox or libretro-thumbnails don't have a match, fall back to ScreenScraper's hash-based lookup. Best-in-class retro coverage but requires API calls and rate-limited.

### Language

**English only** for the initial implementation. Multi-language can be added in Phase 3 without rearchitecting. ScreenScraper is the clear choice if/when multi-language is needed.

### Storage Format

**SQLite + filesystem.** SQLite for text metadata (queryable, single file, efficient). Filesystem for images (natural fit, easy to serve via Axum, easy to clear). Both stored under `<storage_root>/.replay-control/`.

### Data Licensing

All metadata sources used must have licenses compatible with Replay Control's use case (local caching on user's device, no redistribution of data). Key considerations:

| Source | License | Redistribution OK? | Local Cache OK? | Notes |
|--------|---------|---------------------|-----------------|-------|
| **LaunchBox** | Free metadata download, proprietary terms | No (metadata only for personal use) | Yes | Cannot bundle in app binary or redistribute XML |
| **ScreenScraper** | CC for media, community-contributed | Attribution required | Yes | Must credit ScreenScraper in app |
| **libretro-thumbnails** | Community-contributed, freely redistributable | Generally yes | Yes | No explicit license on most repos — treat as fair use for local caching |
| **progetto-SNAPS** | Free for personal use | No redistribution | Yes | Download packs for local use only |
| **Wikidata** | CC0 (public domain) | Yes | Yes | Best license — no restrictions |
| **IGDB** | Twitch Developer Agreement | No long-term caching | Restricted | Data must not be cached indefinitely — problematic for offline-first |
| **Arcade Italia** | Free for scraper use | Attribution expected | Yes | Generous terms for front-end apps |

**Key principles:**
- **Never bundle external metadata in the binary.** Embedded databases (game_db, arcade_db) use our own build pipeline from open DAT files. External metadata is always fetched at runtime and cached locally on the user's device.
- **Attribution in the app.** The Settings/About page should credit metadata sources (ScreenScraper, LaunchBox, libretro-thumbnails, etc.) when their data is cached locally.
- **No redistribution.** The cached `.replay-control/` directory is local to each device. The app never uploads, shares, or serves cached metadata to other devices.
- **Respect rate limits and terms.** API sources (ScreenScraper, IGDB) have usage terms that must be followed — especially thread limits and request quotas.
- **Prefer open/CC0 sources.** When multiple sources provide the same data, prefer the one with the most permissive license (e.g., Wikidata for titles, libretro-thumbnails for images).

---

## Appendix A: API Credential Requirements

| Source | Credentials Needed | How to Obtain |
|--------|--------------------|---------------|
| ScreenScraper | Dev ID/password + User account | Register at screenscraper.fr; request dev credentials |
| IGDB | Twitch Client ID + Secret | Register app at dev.twitch.tv |
| TheGamesDB | API key | Register on forums, request key |
| MobyGames | API key | Paid subscription ($9.99+/mo) |
| RetroAchievements | API key | Free account, key in control panel |
| Arcade Italia | None | Open access |
| libretro-thumbnails | None | Public git repos |
| LaunchBox | None | Download metadata.zip |
| OpenVGDB | None | Download from GitHub |
| Wikidata | None | Open access |
| Hasheous | None | Open access |
| progetto-SNAPS | None | Download from progettosnaps.net |

## Appendix B: Matching Strategy

For games not already identified by the embedded databases, a layered matching approach:

**Step 1: Hash-based identification (most accurate)**
- CRC32 is already available in game_db. MD5/SHA1 can be computed on demand
- For arcade: the zip filename is the MAME ROM name (no hashing needed)
- Query ScreenScraper API with hash(es)

**Step 2: Filename-based fallback**
- Parse ROM filename following No-Intro naming convention: `Game Name (Region) (Languages) (Revision).ext`
- For MAME/FBNeo: the zip filename (without extension) is the ROM name identifier
- Match against LaunchBox XML by cleaned title + platform

**Step 3: Name search fallback**
- Extract clean game name from filename (strip tags, normalize)
- Query IGDB or ScreenScraper by name + platform with fuzzy matching

In practice, step 1 covers the vast majority of games since our embedded databases already identify 25K+ arcade and 34K+ console ROMs. External metadata fetching primarily needs to map these already-identified games to descriptions and images in external sources, which can be done by exact title + platform matching against LaunchBox XML.
