# Game Metadata Sources Research

Research document for external game metadata APIs and databases that can be used in the Replay project to fetch rich metadata (box art, descriptions, ratings, etc.) for retro games.

**Last updated:** March 2026

---

## Summary Comparison Table

| Source | API Type | Auth | Rate Limits | Multi-lang | Retro Coverage | Media Types | Matching | Cost |
|--------|----------|------|-------------|------------|----------------|-------------|----------|------|
| **ScreenScraper** | REST API v2 | Dev credentials + user account | ~50K req/day; thread-limited | EN, FR, ES, DE, IT, PT + more | Excellent (218+ systems) | Box art (2D/3D), screenshots, video, fanart, manual, wheel, marquee | Hash (MD5/SHA1/CRC) + filename | Free (donations for more threads) |
| **IGDB** | REST API v4 | Twitch OAuth2 (Client ID + Secret) | 4 req/sec | Limited (game_localizations endpoint) | Good (all platforms in DB) | Cover art, screenshots, artworks | Name search, platform filter | Free (non-commercial); commercial requires partnership |
| **TheGamesDB** | REST API v2 | API key (forum request) | 3K/month (public) or 6K (private, one-time) | English only | Good (broad platform list) | Box art, screenshots, fanart, banners, clearlogo | Name search, platform filter | Free |
| **MobyGames** | REST API v2 | API key (paid subscription) | 720 req/hr (hobbyist); 1 req/sec max | English only | Excellent (300K+ games, retro strong) | Cover art, screenshots | Name search, platform filter | Paid ($9.99+/mo) |
| **LaunchBox** | XML dump download | None (public download) | N/A (offline data) | English only | Excellent (108K+ games) | Box art, screenshots, 3D boxes, clear logos, banners | Name/title matching | Free (metadata); images via scraping |
| **RetroAchievements** | REST API v1 | API key (free account) | Fair burst limit | English only | Very good (52+ systems, retro-focused) | Game icons, title screens, in-game screenshots, box art | Hash-based (per-system methods) | Free |
| **OpenVGDB** | SQLite DB download | None | N/A (offline data) | English only | Good (many cart-based systems) | Cover art links | ROM hash (MD5/SHA1/CRC) | Free (open source) |
| **Arcade Italia** | REST JSON API | None required | 1 connection/IP recommended | EN, IT (+ lang param) | Arcade-only: 49K+ machines | Screenshots, title screens, marquees, videos, box art | MAME ROM name (zip filename) | Free |
| **No-Intro / DAT-o-Matic** | DAT file downloads | Account (free) | N/A (offline data) | N/A (identification only) | Excellent (console/handheld) | None (identification only) | Hash-based (CRC/MD5/SHA1) | Free |
| **MAME / FBNeo DATs** | DAT file downloads | None | N/A (offline data) | N/A (identification only) | Arcade: comprehensive | None (identification only) | ROM name + hash | Free |
| **Wikidata** | SPARQL + REST API | None (anonymous OK) | Soft limits on SPARQL queries | 400+ languages (labels/descriptions) | Moderate (110K+ games, gaps in retro) | None (links to Wikimedia Commons) | Wikidata ID, external IDs | Free (CC0) |
| **Hasheous** | REST API | None required | Not published (generous) | English (proxies IGDB) | Good (uses No-Intro/Redump/TOSEC/MAME DATs) | Proxies IGDB cover art | Hash-based (MD5/SHA1) | Free (open source) |
| **SteamGridDB** | REST API v2 | API key (free account) | Not published | N/A (artwork only) | Moderate (community-driven) | Grid images, heroes, logos, icons | Name search, Steam App ID | Free |

---

## Detailed Source Analysis

### 1. ScreenScraper (screenscraper.fr)

- **URL:** https://screenscraper.fr/
- **API Docs:** https://screenscraper.fr/webapi2.php (API v2 documentation page)
- **API Type:** REST API v2, returns JSON/XML
- **Authentication:** Requires three levels of credentials:
  1. Developer ID and password (for the application)
  2. Software name and version
  3. User credentials (username + password, registered on screenscraper.fr)
- **Rate Limits:**
  - Up to ~50,000 requests/day total
  - Thread-based system: non-registered users get 0 threads (blocked); registered users get 1 thread; active contributors and donors get more threads (up to 4+)
  - When server load is high, lower-priority users are rate-limited first
- **Multi-language Support:** Best in class for retro databases. Supports at minimum: English (en), French (fr), Spanish (es), German (de), Italian (it), Portuguese (pt). Game descriptions, genres/tags are translated. Language is selectable via API parameter.
- **Systems Coverage:** 218+ systems as of last count. Covers virtually all retro systems: NES, SNES, N64, Game Boy/GBC/GBA, Mega Drive/Genesis, Master System, Saturn, Dreamcast, PlayStation 1/2/PSP, PC Engine, Neo Geo, Amiga, Amstrad CPC, Atari (2600/7800/ST/Lynx/Jaguar), DOS, MAME arcade, FBNeo, MSX, Commodore 64, ZX Spectrum, Vectrex, and many more obscure systems.
- **Media Types:** Box art (2D front/back, 3D rendered), screenshots (in-game), title screens, videos (short gameplay clips), fanart, wheels/logos, marquees, manuals, cartridge/disc media scans, bezels, system overlays. Also provides "mix" composite images combining multiple art types.
- **Matching:** Primary: ROM hash (CRC32, MD5, SHA1 -- tries all three). Fallback: exact filename match (without extension). The hash-based matching leverages an enormous database of verified ROM dumps.
- **Data Fields:** Title, description/synopsis, genre, release date, developer, publisher, number of players, rating, region, language, ROM filename, ROM size, system, family/series.
- **License/Terms:** Free to use. Community-driven under Creative Commons licensing for media. Requires attribution. Donations and contributions encouraged (and grant improved access). No explicit commercial restriction documented, but commercial users should contact the team.
- **Community/Maintenance:** Very actively maintained. Large French-speaking community with global contributors. Regularly updated with new games, media, and system support. Backed by Patreon funding.

**Verdict:** The single best source for retro game metadata due to hash-based matching, multi-language support, and breadth of media types.

---

### 2. IGDB (igdb.com)

- **URL:** https://www.igdb.com/
- **API Docs:** https://api-docs.igdb.com/
- **API Type:** REST API v4, Protobuf or JSON responses. Uses Apicalypse query language in POST body.
- **Authentication:** Requires Twitch Developer account. Register an application at dev.twitch.tv to obtain a Client ID and Client Secret. Authenticate via OAuth2 client credentials flow to get a Bearer token.
- **Rate Limits:** 4 requests/second, max 8 concurrent open requests. Exceeding returns HTTP 429.
- **Multi-language Support:** Limited. A `game_localizations` endpoint exists and is being expanded, primarily providing localized cover art and game names by region. Descriptions/summaries are predominantly English. Multi-language support is a known gap that Twitch/IGDB is working on.
- **Systems Coverage:** Broad platform database covering both modern and retro. Good for major consoles (NES, SNES, N64, PlayStation, etc.) but can have gaps for more obscure retro/computer systems (e.g., less coverage for Amstrad CPC, MSX, or obscure handhelds compared to ScreenScraper).
- **Media Types:** Cover art, screenshots, artworks (promotional art). No videos, manuals, or marquees.
- **Matching:** Name-based search only. No hash or filename-based matching. Search by game name with platform filter. Fuzzy matching available. You query `POST https://api.igdb.com/v4/games` with fields and filters.
- **Data Fields:** Title, summary, storyline, genre, themes, game modes, player perspectives, release dates (per platform/region), developer, publisher, franchise/collection, age ratings, aggregated rating, total rating, cover, screenshots, artworks, videos (YouTube links), websites, similar games, involved companies, platforms.
- **License/Terms:** Free for non-commercial use under Twitch Developer Service Agreement. Commercial use requires a partnership agreement (contact partner@igdb.com). Data cannot be cached indefinitely -- must refresh periodically.
- **Community/Maintenance:** Actively maintained by Twitch/Amazon. Large community of contributors. Very good for modern and well-known retro titles.

**Verdict:** Excellent data richness and well-documented API, but limited multi-language support and no hash-based matching make it a strong secondary source rather than primary for our use case.

---

### 3. TheGamesDB (thegamesdb.net)

- **URL:** https://thegamesdb.net/
- **API Docs:** https://api.thegamesdb.net/
- **API Type:** REST API v2, JSON responses
- **Authentication:** API key required. Must register on the forums and request a key. Two key types:
  - Public key: 1,000 requests/month per unique IP
  - Private key: 6,000 total requests (one-time, never resets)
- **Rate Limits:** Very restrictive. Public key: ~1,000 calls/month. Private key: 6,000 lifetime calls. The response includes `remaining_monthly_allowance` or remaining total for tracking.
- **Multi-language Support:** English only. No multi-language game descriptions or localized metadata.
- **Systems Coverage:** Good breadth: covers major consoles (NES, SNES, N64, GameCube, Wii), Sega systems (Genesis, Saturn, Dreamcast, Master System, Game Gear, SG-1000, 32X, CD), Sony (PS1-PS5, PSP, Vita), Atari, Neo Geo, 3DO, and more. Also covers PC, Mac, and some computers.
- **Media Types:** Box art (front/back, by region), fanart, banners, clearlogo, screenshots, title screens.
- **Matching:** Name search via `/Games/ByGameName` endpoint with optional platform filter. Also search by game ID. No hash-based matching.
- **Data Fields:** Title, overview/description, release date, platform, developer, publisher, genre, players, rating, YouTube link, ESRB rating, cooperative support.
- **License/Terms:** Open database, community-contributed. Free API with registration. Terms are relatively permissive for non-commercial use.
- **Community/Maintenance:** Community-maintained wiki-style database. Had a major redesign. Still active but smaller community than ScreenScraper or IGDB.

**Verdict:** Decent fallback source with good media variety, but very low rate limits and English-only make it unsuitable as a primary source.

---

### 4. MobyGames (mobygames.com)

- **URL:** https://www.mobygames.com/
- **API Docs:** https://www.mobygames.com/info/api/
- **API Type:** REST API v2, JSON responses
- **Authentication:** API key required. Obtained through paid MobyPro subscription.
- **Rate Limits:**
  - Hobbyist ($9.99/mo): 0.2 requests/second
  - Bronze ($99.99/mo): 1 request/second, commercial use
  - Non-commercial legacy keys: 720 req/hr, max 1 req/sec
- **Multi-language Support:** English only for descriptions. MobyGames is an English-language database. No localized game descriptions available.
- **Systems Coverage:** Excellent. 300,000+ game entries across hundreds of platforms. Particularly strong for retro and obscure systems including DOS, Amiga, early computers, and niche consoles.
- **Media Types:** Cover art (front/back, by platform/region), screenshots (in-game), promo art. No videos, manuals, or wheels.
- **Matching:** Name search with platform filter. API endpoints: `/games`, `/games/{id}`, `/games/{id}/platforms/{pid}/screenshots`, `/games/{id}/platforms/{pid}/covers`. No hash-based matching.
- **Data Fields:** Title, description, genre (multi-faceted genre system), release date, developer, publisher, platforms, number of players, perspective, ESRB rating, critic/user ratings, alternate titles.
- **License/Terms:** Paid API access required. Commercial use requires Bronze tier or higher. Data usage subject to MobyGames terms.
- **Community/Maintenance:** One of the oldest and most respected game databases (since 1999). Very actively maintained with high data quality standards. Curated by dedicated community.

**Verdict:** High-quality, comprehensive data -- especially for retro and obscure systems -- but the paid API requirement and English-only descriptions are significant drawbacks.

---

### 5. OpenVGDB

- **URL:** https://github.com/OpenVGDB/OpenVGDB
- **API Type:** Downloadable SQLite database file (no REST API)
- **Authentication:** None required
- **Rate Limits:** N/A (offline database)
- **Multi-language Support:** English only
- **Systems Coverage:** Focused on cartridge-based consoles. Supports NES, SNES, N64, Game Boy/GBC/GBA, Mega Drive/Genesis, Master System, Game Gear, Atari systems, Neo Geo Pocket, WonderSwan, Virtual Boy, GameCube (added in v27), and others. Less coverage for computer platforms and disc-based systems.
- **Media Types:** Cover art image URLs (links to external sources). No screenshots, videos, or other media hosted directly.
- **Matching:** ROM hash-based matching (CRC32, MD5, SHA1). The database maps ROM hashes to game entries. This is the primary lookup method.
- **Data Fields:** Game name, description, region, release date, publisher, developer, genre, ROM filename, ROM size, system. Database tables include ROMs, RELEASES, and CHEATS.
- **License/Terms:** Open source. Free to use and redistribute. Used by OpenEmu (macOS) for game identification.
- **Community/Maintenance:** Actively maintained (latest release v29.0, November 2025). Regular updates adding new games and system support.

**Verdict:** Excellent for offline hash-based ROM identification on cartridge systems. Lightweight and easy to integrate as a local lookup. Limited media and English-only descriptions.

---

### 6. LaunchBox Games Database (gamesdb.launchbox-app.com)

- **URL:** https://gamesdb.launchbox-app.com/
- **API Type:** Downloadable XML/ZIP file (no REST API). Download from https://gamesdb.launchbox-app.com/Metadata.zip
- **Authentication:** None for metadata download. No API keys.
- **Rate Limits:** N/A for the metadata file. Image scraping from the website may be subject to undocumented limits.
- **Multi-language Support:** English only
- **Systems Coverage:** Excellent. 108,000+ games across all major retro and modern platforms. Covers consoles, handhelds, computers, and arcade.
- **Media Types:** Box art (2D front/back, 3D rendered), screenshots (in-game, title screen), clear logos, banners, disc/cartridge media scans. Images are hosted on the LaunchBox website but there is no public API for programmatic image download.
- **Matching:** Name/title-based matching against the XML database. The `Metadata.xml` file contains game names organized by platform. No hash-based matching.
- **Data Fields:** Title (Name), description (Notes), release date, developer, publisher, genre, max players, rating (community rating), video URL, Wikipedia URL, series, cooperative support.
- **License/Terms:** The metadata XML is freely downloadable. However, there is no official public API, and scraping images from the website may violate terms of service. The database content is community-contributed.
- **Community/Maintenance:** Large, active community (LaunchBox is a popular frontend). Database is actively curated and regularly updated.

**Verdict:** Large, well-maintained database with rich data. The lack of a proper API and inability to programmatically download images are major limitations. Best used as an offline reference dataset.

---

### 7. RetroAchievements (retroachievements.org)

- **URL:** https://retroachievements.org/
- **API Docs:** https://api-docs.retroachievements.org/
- **API Type:** REST API v1, JSON responses
- **Authentication:** Free API key from user account control panel. No paid tiers.
- **Rate Limits:** Fair burst limit (exact numbers not published). Users with higher needs can request adjustments via Discord.
- **Multi-language Support:** English only
- **Systems Coverage:** 52+ retro systems. Strong coverage for: NES, SNES, N64, Game Boy/GBC/GBA, Genesis/Mega Drive, Master System, Game Gear, Saturn, Dreamcast, PS1, PS2, PSP, DS, PC Engine, Neo Geo, Atari (2600/7800/Lynx/Jaguar), Amstrad CPC, Arcade, Apple II, MSX, PC-8800, 3DO, Virtual Boy, ColecoVision, Intellivision, Vectrex, and more. Does NOT cover all retro systems (no Amiga, no DOS as of last check).
- **Media Types:** Game icons, title screen screenshots, in-game screenshots, box art. All stored as paths like `/Images/XXXXXX.png` on the RetroAchievements CDN.
- **Matching:** Hash-based matching using system-specific hashing methods. Different systems use different algorithms (e.g., MD5 for most cartridge systems, custom hashes for CD-based systems). The API endpoint `GET /API_GetGameHashes.php?i={gameId}` returns all linked hashes for a game. Also provides a full game list per system with hashes.
- **Data Fields:** Title, console/system, number of achievements, number of players, genre (limited), release date (limited), image paths (icon, title, in-game, box art), rich URL, game ID.
- **License/Terms:** Free to use. API is for community projects. No explicit commercial restrictions documented, but the project is community/volunteer-driven.
- **Community/Maintenance:** Extremely active community. Rapidly growing (reached 1 million achievements milestone). Very well-maintained with regular additions of new systems, games, and achievements.

**Verdict:** Excellent hash-based matching and free API. Game metadata is more limited than dedicated databases (focused on achievements rather than comprehensive game info). Useful as a supplementary source and for ROM identification.

---

### 8. No-Intro / DAT-o-Matic

- **URL:** https://no-intro.org/ and https://datomatic.no-intro.org/
- **API Type:** DAT file downloads (XML/ClrMamePro format). No REST API.
- **Authentication:** Free account required for DAT-o-Matic downloads.
- **Rate Limits:** N/A (file downloads)
- **Multi-language Support:** N/A (ROM identification only, no descriptive metadata)
- **Systems Coverage:** Comprehensive for cartridge-based consoles and handhelds: NES, SNES, N64, Game Boy/GBC/GBA, DS, Mega Drive/Genesis, Master System, Game Gear, Saturn, Atari systems, Neo Geo Pocket, WonderSwan, and many more.
- **Media Types:** None. This is purely a ROM identification database.
- **Matching:** Hash-based (CRC32, MD5, SHA1). DAT files contain verified checksums for every known good dump. ROMs are categorized as verified, not-verified, or bad.
- **Data Fields:** ROM name (following No-Intro naming convention), region, languages, revision, size, CRC32, MD5, SHA1, status (verified/unverified).
- **License/Terms:** Free to download and use. DAT files can be used in ROM managers and identification tools.
- **Community/Maintenance:** Actively maintained by the No-Intro group. Regular DAT updates (daily packs available). The gold standard for console ROM identification and naming.

**Verdict:** Essential for ROM identification and hash-to-name mapping. Not a metadata source itself, but critical infrastructure for matching ROMs to entries in other databases.

---

### 9. MAME / FBNeo DATs

- **MAME URL:** https://www.mamedev.org/ and https://www.progettosnaps.net/dats/MAME/
- **FBNeo URL:** https://github.com/libretro/FBNeo/tree/master/dats
- **API Type:** DAT file downloads (ClrMamePro XML format) and MAME's built-in XML output (`mame -listxml`)
- **Authentication:** None
- **Rate Limits:** N/A (file downloads)
- **Multi-language Support:** N/A (identification and basic metadata only)
- **Systems Coverage:** Arcade: comprehensive. MAME 0.286 covers 49,538+ emulated machines and 139,958+ software list programs. FBNeo covers a subset focused on arcade and select consoles (Neo Geo, CPS1/2/3, etc.).
- **Media Types:** None in the DATs themselves. progetto-SNAPS (progettosnaps.net) provides companion media: snapshots, cabinets, marquees, flyers, icons, artworks, video snaps, manuals. These are downloadable in bulk packs.
- **Matching:** ROM set name (the zip filename, e.g., `pacman.zip`, `sf2.zip`). MAME ROM names are the universal identifier for arcade games. Also includes CRC/SHA1 for individual ROM chips within sets.
- **Data Fields:** Game name, description, year, manufacturer, driver/emulation status, players, cloneof (parent/clone relationships), ROM/CHD file details, software lists.
- **License/Terms:** MAME is open source (GPL/BSD). DATs are freely available. progetto-SNAPS media is community-contributed and freely downloadable.
- **Community/Maintenance:** MAME is one of the most actively developed emulation projects. New versions released monthly with updated DATs.

**Verdict:** The definitive source for arcade game identification and basic metadata. Must be combined with Arcade Italia or ScreenScraper for rich metadata and media.

---

### 10. Arcade Italia / Arcade Database (adb.arcadeitalia.net)

- **URL:** http://adb.arcadeitalia.net/
- **API Docs:** http://adb.arcadeitalia.net/service_scraper.php
- **API Type:** REST JSON API
- **Authentication:** None required
- **Rate Limits:** Recommended: 1 connection per IP at a time (single-threaded). No published hard limits but respectful usage expected.
- **Multi-language Support:** Supports a `lang` parameter. At minimum English (`en`) and Italian (`it`). Other languages may be available.
- **Systems Coverage:** Arcade only. Covers all MAME-emulated machines (49,538+ as of MAME 0.284) plus software lists.
- **Media Types:** Screenshots, title screens, wheels/logos, 2D box art, marquees, videos (VideoSnaps project provides short gameplay clips for most games). Manuals available for some games.
- **Matching:** MAME ROM name (the zip filename). Query via `?game_name=romname` parameter. This is the standard arcade game identifier.
- **Data Fields:** Game name, description (detailed, technical/historical), year, manufacturer, genre, number of players, game type, emulation status, driver info, screen type/orientation, controls, clone/parent relationships, history.
- **License/Terms:** Free to use for scrapers and frontends. Created and maintained by Motoschifo. Attribution appreciated.
- **Community/Maintenance:** Actively maintained by a dedicated individual (Motoschifo). Updated with each MAME release. One of the most comprehensive arcade game databases.
- **Data Export Formats:** XML, CSV, ClrMamePro DAT, HyperSpin XML, EmulationStation XML, Attract-Mode, Excel.

**Verdict:** The best dedicated source for arcade game metadata and media. Excellent complement to MAME DATs. The VideoSnaps project is a unique asset.

---

### 11. Wikidata / Wikipedia

- **URL:** https://www.wikidata.org/ (WikiProject Video Games: https://www.wikidata.org/wiki/Wikidata:WikiProject_Video_games)
- **API Type:** SPARQL endpoint (https://query.wikidata.org/sparql) + REST API (Wikibase REST API)
- **Authentication:** None required for read access (anonymous queries allowed)
- **Rate Limits:** Soft limits on SPARQL query frequency per IP. No published hard numbers. Action API has per-user rate limits.
- **Multi-language Support:** Exceptional. Labels and descriptions available in 400+ languages. All items can have multilingual labels, descriptions, and aliases. This is Wikidata's strongest feature for our use case.
- **Systems Coverage:** Moderate. 110,000+ video game items as of 2024/2025. Good coverage for well-known titles across all eras. Gaps exist for obscure retro titles, regional releases, and homebrew. ~89% have platform data, ~67% have genre, ~50% have developer/publisher.
- **Media Types:** No direct media hosting. Items may link to Wikimedia Commons images (which may have box art, screenshots under free licenses). The connection is indirect.
- **Matching:** By Wikidata item ID (Q-number). Also stores hundreds of external identifiers (IGDB ID, MobyGames ID, ScreenScraper ID, etc.) enabling cross-referencing. No hash-based or filename-based matching.
- **Data Fields (Properties):** Title (label), description, platform (P400), genre (P136), developer (P178), publisher (P123), publication date (P577), game mode (P404), ESRB rating, PEGI rating. Plus 533+ video-game-related external identifiers linking to other databases.
- **License/Terms:** CC0 (public domain dedication). Data is completely free to use for any purpose without attribution (though attribution is appreciated).
- **Community/Maintenance:** Active WikiProject Video Games community. Regular data imports from other databases (e.g., Steam). Continuously growing.

**Verdict:** Unmatched multi-language support and free licensing. Useful as a translation/localization source and as a cross-reference hub linking IDs across databases. Not sufficient as a primary metadata source due to incomplete coverage and lack of media.

---

### 12. Additional Sources

#### Hasheous

- **URL:** https://github.com/gaseous-project/hasheous
- **API Type:** REST API (open source, self-hostable or use community instance)
- **Authentication:** None required
- **Cost:** Free, open source
- **What it does:** Accepts ROM file hashes (MD5, SHA1) and returns matched metadata by cross-referencing No-Intro, Redump, TOSEC, MAME, and MESS DATs. Proxies IGDB for cover art, descriptions, and titles. Also provides RetroAchievements IDs.
- **Verdict:** Excellent middleware service for hash-to-metadata resolution without needing individual API keys for each upstream source.

#### SteamGridDB

- **URL:** https://www.steamgriddb.com/
- **API Docs:** https://www.steamgriddb.com/api/v2
- **API Type:** REST API v2
- **Authentication:** Free API key (account required)
- **What it does:** Community-driven artwork database. Provides grid images, hero banners, logos, and icons. Not limited to Steam games -- covers many retro titles.
- **Verdict:** Useful supplementary source for high-quality artwork/logos. Not a metadata source (no descriptions, genres, etc.).

#### Libretro Database

- **URL:** https://github.com/libretro/libretro-database
- **API Type:** Downloadable RDB files + DATs
- **What it does:** Compiles No-Intro, Redump, MAME, and TOSEC DATs into RetroArch's .rdb format. Used for ROM validation, naming, and thumbnail matching. CRC-based matching for cartridge games, serial-based for disc games.
- **Verdict:** Useful for ROM identification and as a reference for hash databases. The RDB format is RetroArch-specific but the underlying DATs are universal.

#### progetto-SNAPS

- **URL:** https://www.progettosnaps.net/
- **What it does:** Provides bulk-downloadable MAME media packs: snapshots, cabinets, marquees, flyers, icons, artworks, video snaps, manuals, PCB scans. Updated with each MAME release.
- **Verdict:** Essential companion to MAME DATs for arcade media. Not an API -- bulk download packs only.

---

## Multi-Language Support Analysis

Multi-language metadata is critical for Replay. Here is the ranking of sources by language support:

### Tier 1: Excellent Multi-Language Support
1. **Wikidata** -- Labels and descriptions in 400+ languages. Best source for translating game titles and short descriptions into Spanish, French, German, Japanese, and virtually any other language. However, descriptions are brief (one line) rather than full synopses.
2. **ScreenScraper** -- Game descriptions available in at least English, French, Spanish, German, Italian, and Portuguese. The best source for full multi-language game synopses/descriptions among dedicated game databases.

### Tier 2: Limited Multi-Language Support
3. **IGDB** -- Has a `game_localizations` endpoint being expanded. Primarily English with some regional cover art variants. Localized descriptions are sparse.
4. **Arcade Italia** -- Supports English and Italian via `lang` parameter. Limited to two languages.

### Tier 3: English Only
5. **TheGamesDB** -- English only
6. **MobyGames** -- English only
7. **LaunchBox** -- English only
8. **RetroAchievements** -- English only
9. **OpenVGDB** -- English only

### Recommended Multi-Language Strategy

For the Replay project's English + Spanish minimum requirement:

1. **Primary descriptions (EN/ES/FR/DE/IT/PT):** ScreenScraper -- provides the most languages for full game descriptions.
2. **Game title translations:** Wikidata -- use SPARQL queries to fetch labels in target language. Since Wikidata uses external identifiers, you can cross-reference ScreenScraper IDs, IGDB IDs, etc.
3. **Fallback for untranslated descriptions:** IGDB for English descriptions, with Wikidata short descriptions as a last resort.

---

## Recommendations

### Primary Source: Console / Handheld / Computer Games

**ScreenScraper** is the recommended primary source because:
- Hash-based matching (MD5/SHA1/CRC) works directly with ROM files -- no need to parse filenames
- Multi-language descriptions (EN, ES, FR, DE, IT, PT)
- Richest media collection (box art, screenshots, videos, manuals, wheels, marquees)
- 218+ systems covering virtually all retro platforms relevant to RePlayOS
- Free to use (with registration)
- Community-driven with active maintenance

### Primary Source: Arcade Games

**Arcade Italia (adb.arcadeitalia.net)** combined with **MAME DATs** is recommended:
- Arcade Italia provides rich metadata, screenshots, videos, and detailed descriptions keyed by MAME ROM name
- MAME DATs provide the definitive ROM identification and parent/clone relationships
- Together they cover 49,000+ arcade machines comprehensively
- Free, no API key required

**ScreenScraper** is also excellent for arcade and can serve as the unified source if a single-source approach is preferred.

### Fallback Sources (Priority Order)

1. **IGDB** -- Good general game data, well-documented API, free for non-commercial use. Use when ScreenScraper has no match.
2. **TheGamesDB** -- Additional media types (fanart, banners). Use sparingly due to low rate limits.
3. **RetroAchievements** -- Hash-based matching provides alternative ROM identification. Useful cross-reference for game icons and achievement counts.
4. **Hasheous** -- Free hash-based identification middleware that proxies multiple sources. Good for batch identification without managing multiple API keys.
5. **LaunchBox XML** -- Offline fallback with 108K+ games. Good for filling gaps when online APIs are unavailable.

### Recommended Matching Strategy

A layered approach is recommended:

**Step 1: Hash-based identification (most accurate)**
- Compute CRC32, MD5, and SHA1 of each ROM file
- For arcade: use the zip filename as the MAME ROM name
- Query ScreenScraper API with all three hashes
- Cross-reference with No-Intro/Redump DATs (via Hasheous or local DB) for game name normalization

**Step 2: Filename-based fallback**
- If hash matching fails, parse the ROM filename following No-Intro naming convention: `Game Name (Region) (Languages) (Revision).ext`
- For MAME/FBNeo: the zip filename (without extension) is the ROM name identifier
- Query ScreenScraper by filename (they support this as a fallback)

**Step 3: Name search fallback**
- Extract the clean game name from the filename
- Query IGDB or TheGamesDB by name + platform
- Apply fuzzy matching to handle naming variations

**Step 4: Metadata enrichment**
- Once a game is identified, fetch descriptions in multiple languages from ScreenScraper
- Supplement with Wikidata for additional language translations
- Fetch additional artwork from IGDB (artworks, alternative covers)

### Data Architecture Suggestion

- **Local cache:** Store a local SQLite database with No-Intro, Redump, and MAME DATs for fast hash-based lookups
- **API calls:** Use ScreenScraper as the primary API, falling back to IGDB
- **Media cache:** Download and cache images locally to avoid repeated API calls
- **Identifier cross-reference:** Store multiple external IDs per game (ScreenScraper ID, IGDB ID, Wikidata Q-number, RetroAchievements ID) to enable future cross-referencing

---

## API Credential Requirements Summary

| Source | Credentials Needed | How to Obtain |
|--------|--------------------|---------------|
| ScreenScraper | Dev ID/password + User account | Register at screenscraper.fr; request dev credentials |
| IGDB | Twitch Client ID + Secret | Register app at dev.twitch.tv |
| TheGamesDB | API key | Register on forums.thegamesdb.net, request key |
| MobyGames | API key | Paid subscription at mobygames.com ($9.99+/mo) |
| RetroAchievements | API key | Free account, key in control panel |
| Arcade Italia | None | Open access |
| SteamGridDB | API key | Free account at steamgriddb.com |
| Hasheous | None | Open access |
| Wikidata | None | Open access (anonymous) |
| OpenVGDB | None | Download from GitHub |
| LaunchBox | None | Download metadata.zip |
| No-Intro | Account | Free registration at datomatic.no-intro.org |
