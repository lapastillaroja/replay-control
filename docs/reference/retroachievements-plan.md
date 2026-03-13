# RetroAchievements Integration Plan

> Research date: 2026-03-13
> Source: [RetroAchievements API docs](https://api-docs.retroachievements.org/) ([GitHub](https://github.com/RetroAchievements/api-docs))

## 1. RA API Overview

### Authentication

RA uses a **Web API key** (not OAuth, not username/password). Each RA account gets a key from their [control panel](https://retroachievements.org/controlpanel.php). Every API request passes it as the `y` query parameter:

```
https://retroachievements.org/API/API_GetGame.php?i=1&y=YOUR_API_KEY
```

There is also a **Connect API** used by emulators (`dorequest.php`), which uses a separate token obtained via username+password. We do NOT need the Connect API -- it is for emulator integrations that award achievements. Our app is read-only (a companion viewer), so the Web API is sufficient.

**What we need from the user:** Their RA username + their Web API key (both available from their RA profile/control panel). No password needed.

### Rate Limits

RA has rate limiting enabled but does not publish exact numbers. Their guidelines say:

- "Just because you _can_ hit the API for N requests a second does not mean that you should."
- Game data is mostly static -- cache aggressively.
- The `GetGameList` endpoint warns: "consider aggressively caching this endpoint's response" (response can be huge for some systems).
- If the default rate limit is insufficient, they invite contacting them on Discord.

**Practical implication:** We must cache everything in SQLite and avoid per-page-load API calls. Background sync with conservative intervals.

### Base URL

All Web API endpoints: `https://retroachievements.org/API/API_*.php`

### Key Endpoints

#### User endpoints

| Endpoint | URL | Purpose |
|---|---|---|
| **User Profile** | `API_GetUserProfile.php?u={user}` | Basic info: ID, ULID, avatar, points, rank, motto |
| **User Points** | `API_GetUserPoints.php?u={user}` | Hardcore + softcore point totals |
| **User Awards** | `API_GetUserAwards.php?u={user}` | Mastery/beaten/completion awards list |
| **User Recent Achievements** | `API_GetUserRecentAchievements.php?u={user}&m={minutes}` | Recently unlocked achievements (default: last 60 min) |
| **User Completion Progress** | `API_GetUserCompletionProgress.php?u={user}&c={count}&o={offset}` | Per-game completion stats (paginated, max 500) |
| **User Recently Played** | `API_GetUserRecentlyPlayedGames.php?u={user}&c={count}` | Last played games with progress stats |
| **User Progress (batch)** | `API_GetUserProgress.php?u={user}&i={gameId1,gameId2,...}` | Summary progress for specific game IDs (batch lookup) |

#### Game endpoints

| Endpoint | URL | Purpose |
|---|---|---|
| **Game Summary** | `API_GetGame.php?i={gameId}` | Title, console, images, publisher, developer, genre |
| **Game Extended** | `API_GetGameExtended.php?i={gameId}` | Full metadata + all achievements with descriptions/points |
| **Game Hashes** | `API_GetGameHashes.php?i={gameId}` | All ROM hashes (MD5) linked to a game |
| **Game Progression** | `API_GetGameProgression.php?i={gameId}` | Median time to beat/master, per-achievement unlock times |
| **Game Info + User Progress** | `API_GetGameInfoAndUserProgress.php?g={gameId}&u={user}` | Combined: game metadata + user's per-achievement unlock status |
| **Achievement Count** | `API_GetAchievementCount.php?i={gameId}` | List of achievement IDs for a game (useful for revision detection) |

#### System endpoints

| Endpoint | URL | Purpose |
|---|---|---|
| **All Systems** | `API_GetConsoleIDs.php?a=1&g=1` | List of all system IDs + names |
| **System Game List** | `API_GetGameList.php?i={systemId}&f=1&h=1` | All games for a system with hashes (heavy -- cache aggressively) |

### Response Format

All responses are JSON. Image URLs are relative paths (e.g., `/Images/067895.png`) served from `https://retroachievements.org` or `https://media.retroachievements.org`. Achievement badge URLs follow the pattern `/Badge/{badgeName}.png`.

### Username vs ULID

As of 2025, RA users can change their usernames. The ULID (Universally Unique Lexicographically Sortable Identifier) is stable. Best practice: query by username initially, then store the ULID for future requests. Both are accepted in the `u` parameter.

---

## 2. RA Platform IDs and System Mapping

RA uses numeric `ConsoleID` values. These are the IDs for systems RePlayOS supports:

| RA ConsoleID | RA Name | RePlayOS folder | RePlayOS display name |
|---|---|---|---|
| 1 | Mega Drive / Genesis | `sega_smd` | Sega Mega Drive / Genesis |
| 2 | Nintendo 64 | `nintendo_n64` | Nintendo 64 |
| 3 | SNES/Super Famicom | `nintendo_snes` | Super Nintendo |
| 4 | Game Boy | `nintendo_gb` | Game Boy |
| 5 | Game Boy Advance | `nintendo_gba` | Game Boy Advance |
| 6 | Game Boy Color | `nintendo_gbc` | Game Boy Color |
| 7 | NES/Famicom | `nintendo_nes` | Nintendo Entertainment System |
| 10 | Atari 2600 | `atari_2600` | Atari 2600 |
| 11 | Sega Master System | `sega_sms` | Sega Master System |
| 12 | PlayStation | `sony_psx` | PlayStation |
| 13 | Atari Lynx | `atari_lynx` | Atari Lynx |
| 15 | Sega Game Gear | `sega_gg` | Sega Game Gear |
| 17 | Atari Jaguar | `atari_jaguar` | Atari Jaguar |
| 21 | PlayStation 2 | `sony_ps2` | PlayStation 2 |
| 25 | Atari 2600 | (duplicate -- verify) | |
| 27 | Arcade | `arcade_fbneo` / `arcade_mame` | Arcade (FBNeo/MAME) |
| 33 | Sega 32X | `sega_32x` | Sega 32X |
| 37 | Amstrad CPC | `amstrad_cpc` | Amstrad CPC |
| 39 | Sega Saturn | `sega_st` | Sega Saturn |
| 40 | Sega Dreamcast | `sega_dc` | Sega Dreamcast |
| 41 | PSP | `sony_psp` | PlayStation Portable |
| 43 | 3DO | `panasonic_3do` | 3DO |
| 51 | Atari 7800 | `atari_7800` | Atari 7800 |
| 9 | Sega CD | `sega_cd` | Sega CD / Mega-CD |
| ?? | SG-1000 | `sega_sg` | Sega SG-1000 |

**Note:** The mapping needs to be verified by calling `API_GetConsoleIDs.php?a=1&g=1` once and cross-referencing. A static mapping table should be compiled into the binary (like the arcade DB), not fetched at runtime.

**Systems NOT in RA:** Some RePlayOS systems may not have RA support (e.g., ScummVM, Sharp X68000, Philips CD-i). For these, the achievement features simply don't appear.

---

## 3. Game Matching Strategy

### How RA Identifies Games

RA identifies games by **MD5 hash of the ROM file**. Each game entry has one or more linked hashes (via `API_GetGameHashes`). The `API_GetGameList` endpoint can also return hashes inline when `h=1` is set.

### Hash-Based Matching (Primary Strategy)

1. **Compute MD5 hash** of each ROM file on disk
2. **Look up the hash** against the RA database to get the RA Game ID
3. Once we have the Game ID, we can fetch achievements, user progress, etc.

**Advantages:**
- Precise -- identifies the exact ROM version
- Handles renamed files, different naming conventions, translations, etc.
- RA already maintains the hash database

**Challenges:**
- Must compute MD5 of potentially large ROM files (CHD, ISO = hundreds of MB to GB)
- Arcade ROMs: RA expects the hash of the ROM zip (or specific files within it?) -- needs testing
- Not all ROMs will have a matching hash (ROM hacks, rare dumps, non-standard formats)
- RA may use a custom hashing algorithm for some systems (e.g., stripping headers from NES ROMs)

### Title-Based Matching (Fallback)

For ROMs whose hash is not found in RA:

1. Parse the ROM filename to extract the clean game title (we already have this via `rom_tags.rs`)
2. Fuzzy-match against the RA game titles for the same system
3. Present as "possible match" rather than confirmed

This is the same fuzzy strategy we use for thumbnail matching (`thumbnails.rs`), and we can reuse the normalization logic.

### Recommended Approach

**Phase 1 (MVP):** Title-based matching only. Much simpler to implement -- no ROM hashing needed. Use the `API_GetGameList` endpoint to build a mapping of RA game titles per system, then match against our ROM filenames using our existing title normalization. Store the resolved RA Game ID in the database.

**Phase 2 (Enhancement):** Add hash-based matching for definitive identification. Compute ROM MD5 hashes as a background task (similar to how we index thumbnails). Hash computation can be batched and cached. Fall back to title match when hash is not found.

### Building the RA Game Index

Fetch `API_GetGameList.php?i={systemId}&f=1` for each supported system (only games with achievements). Store in SQLite. This is a one-time bulk fetch per system, refreshed periodically (weekly/monthly). The endpoint warns about bandwidth -- cache the response and store it locally.

---

## 4. Data Model

### New SQLite Tables (in `user_data.db`)

Achievement data is user-specific and should survive metadata clears, so it belongs in `user_data.db` (not `metadata.db`).

```sql
-- RA system ID mapping (static, populated on first run)
CREATE TABLE ra_systems (
    replayos_folder TEXT PRIMARY KEY,  -- e.g., "nintendo_snes"
    ra_console_id   INTEGER NOT NULL,  -- e.g., 3
    ra_console_name TEXT NOT NULL      -- e.g., "SNES/Super Famicom"
);

-- RA game index per system (from GetGameList, refreshed periodically)
CREATE TABLE ra_games (
    ra_game_id       INTEGER PRIMARY KEY,
    ra_console_id    INTEGER NOT NULL,
    title            TEXT NOT NULL,
    title_normalized TEXT NOT NULL,     -- lowercase, stripped for fuzzy matching
    image_icon       TEXT,              -- relative URL to RA icon
    num_achievements INTEGER NOT NULL DEFAULT 0,
    points           INTEGER NOT NULL DEFAULT 0,
    updated_at       TEXT NOT NULL      -- ISO8601 timestamp of last refresh
);

-- Hash-to-game mapping (from GetGameList with h=1, or GetGameHashes)
CREATE TABLE ra_hashes (
    md5        TEXT PRIMARY KEY,
    ra_game_id INTEGER NOT NULL REFERENCES ra_games(ra_game_id)
);

-- Resolved mapping: our ROM -> RA game
CREATE TABLE ra_rom_mapping (
    system       TEXT NOT NULL,        -- replayos folder name
    rom_filename TEXT NOT NULL,        -- ROM filename
    ra_game_id   INTEGER,             -- resolved RA game ID (NULL if unmatched)
    match_method TEXT NOT NULL,        -- "hash", "title_exact", "title_fuzzy", "manual"
    rom_md5      TEXT,                 -- computed hash (NULL if not yet computed)
    updated_at   TEXT NOT NULL,
    PRIMARY KEY (system, rom_filename)
);

-- User's achievement progress per RA game
CREATE TABLE ra_user_progress (
    ra_game_id            INTEGER PRIMARY KEY,
    num_achievements      INTEGER NOT NULL,
    num_awarded           INTEGER NOT NULL DEFAULT 0,
    num_awarded_hardcore  INTEGER NOT NULL DEFAULT 0,
    score_possible        INTEGER NOT NULL DEFAULT 0,
    score_achieved        INTEGER NOT NULL DEFAULT 0,
    highest_award_kind    TEXT,         -- "mastered", "beaten-hardcore", "beaten-softcore", NULL
    highest_award_date    TEXT,
    updated_at            TEXT NOT NULL
);

-- Individual achievement details + unlock status
CREATE TABLE ra_achievements (
    ra_achievement_id  INTEGER PRIMARY KEY,
    ra_game_id         INTEGER NOT NULL,
    title              TEXT NOT NULL,
    description        TEXT NOT NULL,
    points             INTEGER NOT NULL,
    true_ratio         INTEGER NOT NULL DEFAULT 0,
    badge_name         TEXT,           -- badge image identifier
    display_order      INTEGER NOT NULL DEFAULT 0,
    achievement_type   TEXT,           -- "progression", "win_condition", "missable", NULL
    date_earned        TEXT,           -- NULL if not earned
    date_earned_hc     TEXT,           -- NULL if not earned hardcore
    updated_at         TEXT NOT NULL
);

-- Sync metadata
CREATE TABLE ra_sync_meta (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL
);
-- Keys: "ra_username", "ra_ulid", "last_profile_sync", "last_progress_sync",
--        "game_index_last_sync_{system}", etc.
```

### Settings (in `settings.cfg`)

```
ra_username = "MyUsername"
ra_api_key = "ABC123secretkey"
```

The API key is stored on-device in `settings.cfg` (same as `github_api_key`). This is acceptable because:
- The device is on the user's local network
- The API key is read-only (cannot award achievements or modify the RA account)
- Same security posture as the existing GitHub API key

### Refresh Strategy

| Data | Refresh interval | Trigger |
|---|---|---|
| RA game index (per system) | Weekly or manual | Background task on startup, or "Sync" button |
| User profile | On settings page load | When viewing RA profile section |
| Per-game achievements + progress | On game detail page load | Lazy fetch per game, cached for 1 hour |
| ROM-to-RA mapping | On first visit + when new ROMs added | Part of ROM scan, cached permanently |

**Fetch approach:** Per-game lazy fetching (not batch). When the user opens a game detail page, fetch that game's achievements + progress from RA (using `GetGameInfoAndUserProgress` — a single call). Cache the result for 1 hour. This avoids the complexity and rate-limit risk of batch-fetching progress for entire systems at once.

> **Why no batch fetch:** The `GetUserProgress` batch endpoint accepts comma-separated game IDs, but the practical limits are unclear and it's easy to hit rate limits with large libraries. Lazy per-game fetching is simpler, generates fewer API calls overall (most games are never viewed), and always gives fresh data when the user actually cares about it. Batch sync can be reconsidered in Phase 3 if users want system-wide progress overviews.

---

## 5. Feature Proposals

### 5.1 Settings Page: RA Account Setup

Add a new section to the existing `/more` page or a new `/more/retroachievements` page.

```
┌──────────────────────────────────┐
│ RetroAchievements                │
│                                  │
│ Username                         │
│ ┌──────────────────────────────┐ │
│ │ MyRaUsername                 │ │
│ └──────────────────────────────┘ │
│                                  │
│ Web API Key                      │
│ ┌──────────────────────────────┐ │
│ │ ●●●●●●●●●●●●●●●●           │ │
│ └──────────────────────────────┘ │
│ Get your key at                  │
│ retroachievements.org/controlpan │
│                                  │
│ ┌──────────┐  ┌──────────────┐  │
│ │   Save   │  │  Test Login  │  │
│ └──────────┘  └──────────────┘  │
│                                  │
│ ── Account Info ──────────────── │
│ Points: 12,450 (HC) / 200 (SC)  │
│ Rank: #4,616                     │
│ Member since: 2021-12-20        │
│ Awards: 24 beaten, 6 mastered   │
└──────────────────────────────────┘
```

**"Test Login"** calls `API_GetUserProfile` with the provided credentials to verify they work, then displays the account summary.

### 5.2 Game Detail Page: Achievement List

On the existing `/games/:system/:filename` page, add an "Achievements" section below the existing metadata.

```
┌──────────────────────────────────┐
│ Super Mario World                │
│ SNES · 1990 · Nintendo           │
│ ┌─────────┐                      │
│ │ box art │  Genre: Platformer   │
│ │         │  Players: 1-2        │
│ └─────────┘  Rating: 4.5/5       │
│                                  │
│ ── Achievements ─── 45/89 (50%) ─│
│                                  │
│ ▓▓▓▓▓▓▓▓▓▓░░░░░░░░░░ 50%       │
│                                  │
│ ✓ Giddy Up!              3 pts  │
│   Catch a ride with a friend     │
│   Earned: 2024-03-15             │
│                                  │
│ ✓ Unleash The Dragon      2 pts  │
│   Collect 5 Dragon Coins...      │
│   Earned: 2024-03-15             │
│                                  │
│ ○ Secret Exit Master     10 pts  │
│   Find all 24 secret exits       │
│                                  │
│ ○ 100% Completion        25 pts  │
│   Complete all 96 exits          │
│                                  │
│ ── Time to Beat ──────────────── │
│ Median: 4h 58m                   │
│ Median to Master: 22h 9m        │
│                                  │
│ View on RetroAchievements →      │
└──────────────────────────────────┘
```

**States:**
- **No RA account configured:** Show "Connect your RetroAchievements account in Settings to see achievements" with a link
- **RA account configured, game matched:** Show achievement list with progress
- **RA account configured, game not matched:** Show "This game was not found in the RetroAchievements database"
- **RA account configured, game has no achievements:** Show "No achievements available for this game yet"
- **Loading:** Show skeleton/shimmer

Achievement types from RA can be visually distinguished:
- `progression` -- standard story/level achievements
- `win_condition` -- game completion
- `missable` -- can be missed, highlight with a warning icon

### 5.3 Games List: Achievement Badge

> **Design note:** The current game list already shows a lot of information (box art, genre, players, rating icons, file size). Adding achievement progress risks visual overload. This needs careful design iteration — likely we should first **simplify** the game list (remove or rethink some of the existing info density) before adding achievement data.

**Phase 1 approach:** Add a minimal, non-intrusive indicator — a small trophy icon or dot that indicates "has achievements" / "in progress" / "mastered", without adding text or progress bars. Full progress bars can come later after the game list design is revisited.

**Game list redesign (separate task, prerequisite):**
- Audit what information is currently shown per ROM card and whether all of it is useful
- Consider what users actually scan for when browsing a system (title + box art are primary)
- Consider making secondary info (genre, players, file size) expandable or hidden behind a tap
- Only then layer in achievement indicators that complement the simplified layout

Minimal indicators:
- Small gold star icon: Mastered
- Small green check icon: Beaten
- Small trophy icon with count: Has achievements, partial or no progress
- No indicator: Game not in RA

### 5.4 Home Page: Recent Achievements

On the `/` home page, add a section for recent achievements (below the existing "Recently Played" section).

```
┌──────────────────────────────────┐
│ ── Recent Achievements ───────── │
│                                  │
│ 🏆 That Was Easy          3 pts │
│    Sonic the Hedgehog · MD       │
│    2 hours ago                   │
│                                  │
│ 🏆 Giddy Up!              3 pts │
│    Super Mario World · SNES      │
│    Yesterday                     │
│                                  │
│ ── Achievement Stats ─────────── │
│ Total Points: 12,450             │
│ Games Beaten: 24                 │
│ Games Mastered: 6                │
│ Global Rank: #4,616              │
└──────────────────────────────────┘
```

This data comes from `API_GetUserRecentAchievements` (with a large lookback window, e.g., `m=10080` for 7 days) and `API_GetUserProfile`.

### 5.5 Creative Ideas (Phase 3+)

> **Note:** These are deferred to Phase 3+ to keep the home page focused. The home page already has several sections (Top Rated, Random, Recently Played). Adding too many achievement sections risks clutter. These should be added incrementally after evaluating the home page layout holistically.

**"Unstarted games with achievements"** -- Cross-reference the user's ROM library with RA game index. Show games that have achievements but the user has 0% progress. Great for discovery. *Deferred to Phase 3.*

**"Close to mastery"** -- Games where the user is >75% complete. Motivational nudge. *Deferred to Phase 3.*

**"Achievement of the week"** -- Show the current RA community achievement of the week (from `API_GetAchievementOfTheWeek`). Light community connection. *Deferred to Phase 3.*

**Design consideration:** Before adding these sections, evaluate the home page holistically. It may make more sense to create a dedicated `/achievements` page that aggregates all RA data (profile stats, recent achievements, unstarted games, close to mastery) rather than scattering sections across the home page.

---

## 6. Server Function Design

### New Server Functions

```rust
// Settings
#[server]
async fn GetRaCredentials() -> Result<Option<RaCredentials>, ServerFnError> { ... }

#[server]
async fn SaveRaCredentials(username: String, api_key: String) -> Result<RaProfile, ServerFnError> { ... }

#[server]
async fn ClearRaCredentials() -> Result<(), ServerFnError> { ... }

// Profile
#[server]
async fn GetRaProfile() -> Result<Option<RaProfile>, ServerFnError> { ... }

// Game achievements (for game detail page)
#[server]
async fn GetGameAchievements(system: String, filename: String) -> Result<Option<GameAchievementInfo>, ServerFnError> { ... }

// Batch progress (for games list)
#[server]
async fn GetSystemAchievementProgress(system: String) -> Result<Vec<RomAchievementSummary>, ServerFnError> { ... }

// Home page
#[server]
async fn GetRecentAchievements() -> Result<Vec<RecentAchievement>, ServerFnError> { ... }
#[server]
async fn GetRaStats() -> Result<Option<RaUserStats>, ServerFnError> { ... }

// Sync
#[server]
async fn SyncRaGameIndex(system: String) -> Result<SyncResult, ServerFnError> { ... }
#[server]
async fn SyncRaUserProgress() -> Result<SyncResult, ServerFnError> { ... }

// Discovery
#[server]
async fn GetUnstartedGamesWithAchievements() -> Result<Vec<UnstartedGame>, ServerFnError> { ... }
```

### Data Types (shared between server and client)

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RaCredentials {
    pub username: String,
    pub has_api_key: bool,  // never send the key to the client
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RaProfile {
    pub username: String,
    pub ulid: String,
    pub avatar_url: String,
    pub total_points: u32,
    pub total_softcore_points: u32,
    pub total_true_points: u32,
    pub rank: u32,
    pub member_since: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GameAchievementInfo {
    pub ra_game_id: u32,
    pub game_title: String,
    pub num_achievements: u32,
    pub num_awarded: u32,
    pub num_awarded_hardcore: u32,
    pub completion_pct: f32,
    pub highest_award: Option<String>,  // "mastered", "beaten-hardcore", etc.
    pub achievements: Vec<Achievement>,
    pub median_time_to_beat: Option<u64>,     // seconds
    pub median_time_to_master: Option<u64>,   // seconds
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Achievement {
    pub id: u32,
    pub title: String,
    pub description: String,
    pub points: u32,
    pub badge_url: String,
    pub achievement_type: Option<String>,
    pub date_earned: Option<String>,
    pub date_earned_hardcore: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RomAchievementSummary {
    pub rom_filename: String,
    pub ra_game_id: Option<u32>,
    pub num_achievements: u32,
    pub num_awarded: u32,
    pub completion_pct: f32,
    pub highest_award: Option<String>,
}
```

### HTTP Client

Use `hyper` (lightweight, already a transitive dependency via axum) instead of `reqwest` to avoid pulling in a heavy dependency. Since RA API calls are simple GET requests with JSON responses, hyper's client is sufficient:

```rust
// In replay-control-core/src/retroachievements/client.rs
pub struct RaClient {
    http: hyper_util::client::legacy::Client<
        hyper_rustls::HttpsConnector<hyper_util::client::legacy::connect::HttpConnector>,
        http_body_util::Empty<bytes::Bytes>,
    >,
    api_key: String,
}

impl RaClient {
    pub fn new(api_key: String) -> Self { ... }
    pub async fn get_user_profile(&self, username: &str) -> Result<RaUserProfile> { ... }
    pub async fn get_game_info_and_progress(&self, game_id: u32, username: &str) -> Result<...> { ... }
    // etc.
}
```

**Why hyper over reqwest:** hyper is already in the dependency tree (via axum/leptos), so no new dependency needed. reqwest adds ~30 crates and increases compile time. The RA API is simple REST (GET + JSON) with no cookies, redirects, or multipart — hyper is more than sufficient.

### Registration Pattern

All server functions from the library crate need `register_explicit` in `main.rs` (same as metadata server functions):

```rust
server_fn::axum::register_explicit::<GetRaProfile>();
server_fn::axum::register_explicit::<GetGameAchievements>();
// ... etc
```

---

## 7. Implementation Phases

### Phase 1: MVP (Core Read-Only Integration)

**Goal:** Users can connect their RA account and see achievement progress on game detail pages.

1. **RA settings UI** (`/more/retroachievements`)
   - Username + API key input fields
   - "Test & Save" button that validates credentials via `GetUserProfile`
   - Display basic profile info on success
   - Store in `settings.cfg` as `ra_username` and `ra_api_key`

2. **RA client module** (`replay-control-core/src/retroachievements/`)
   - HTTP client wrapper for RA API
   - Deserialization types for RA responses
   - Error handling (invalid key, rate limited, network error)

3. **System ID mapping**
   - Static `HashMap<&str, u32>` mapping RePlayOS folder names to RA console IDs
   - Compiled into the binary (like arcade DB)

4. **Game detail achievements** (`/games/:system/:filename`)
   - On page load (if RA configured), call `GetGameInfoAndUserProgress` with title-matched game ID
   - Title matching: normalize game title from ROM filename, search against RA game list for the system
   - Display achievement list with earned/unearned status
   - Cache result in `ra_achievements` + `ra_user_progress` tables

5. **RA game index sync**
   - Fetch `GetGameList` per system and store in `ra_games` table
   - Run once on first RA setup, then weekly (or manual refresh button)
   - Title normalization for matching

**Estimated effort:** 3-5 days

### Phase 2: Progress Overview

**Goal:** Achievement progress visible across the app, not just on individual game pages.

1. **Home page achievements section**
   - Recent achievements via `GetUserRecentAchievements` (lookback: 7 days)
   - RA stats summary from cached profile data

2. **Games list: minimal achievement indicators**
   - Small trophy/star/check icons per ROM card (not progress bars)
   - Uses cached data from game detail visits (no batch sync)
   - **Prerequisite:** Game list design audit — currently shows too much info per card. Simplify first, then layer in achievement indicators.

3. **Background sync** (conservative)
   - Refresh profile stats periodically (not per-game progress)
   - Sync indicator in UI ("Last synced: 5 min ago")

**Estimated effort:** 2-3 days

### Phase 3: Discovery & Polish

**Goal:** Use RA data for discovery features and polish the experience.

1. **Hash-based matching**
   - Background MD5 computation for ROM files
   - Store in `ra_rom_mapping.rom_md5`
   - Hash lookup against `ra_hashes` table for definitive matching

2. **Dedicated achievements page** (`/achievements`)
   - Aggregated view: profile stats, recent achievements, unstarted games, close to mastery
   - Better than scattering sections across the home page

3. **Unstarted games with achievements**
   - Cross-reference library with RA game index
   - Show on dedicated achievements page

4. **Close to mastery**
   - Filter games where user progress is >75%
   - Show on dedicated achievements page

5. **Achievement images**
   - Cache RA badge images locally (similar to box art caching)
   - Serve via the existing media handler

6. **Manual game matching**
   - For games that don't auto-match, allow user to manually link a ROM to an RA game
   - Search RA game list by title, select the correct match
   - Store as `match_method = "manual"` in `ra_rom_mapping`

**Estimated effort:** 3-4 days

### Phase 4: Future Ideas

- **Achievement notifications** -- if RetroArch on RePlayOS is configured with RA, detect newly earned achievements and show them in the app
- **Leaderboards** -- show game leaderboards from `GetGameLeaderboards`
- **Community stats** -- "X% of players earned this achievement" rarity indicators
- **Achievement guides** -- link to RA game guides via `GuideURL` field
- **Export/share** -- generate a shareable achievement progress summary

---

## 8. Edge Cases

### No RA Account Configured

- All achievement UI sections are hidden or show a soft prompt: "Connect your RetroAchievements account in Settings to track achievements"
- No API calls are made
- No performance impact on the rest of the app

### Game Not in RA Database

- Title matching returns no result
- Game detail page shows: "This game is not tracked on RetroAchievements" (neutral tone)
- No achievement section shown
- The ROM is stored in `ra_rom_mapping` with `ra_game_id = NULL` to avoid re-querying

### Game Has No Achievements

- Game is in RA but has 0 achievements (some games are registered but have no achievement set yet)
- Show: "No achievements available for this game on RetroAchievements"

### RA API Unavailable / Network Error

- The Pi may have no internet access (common for isolated retro gaming setups)
- All RA data is cached in SQLite -- show cached data with a "Last synced" timestamp
- If no cached data exists and API is unreachable, show: "RetroAchievements data unavailable -- check your internet connection"
- Never block page rendering on RA API calls -- always load the page first, then hydrate achievement data asynchronously

### NFS Mount Scenarios

- `user_data.db` is on the NFS mount -- same `nolock` VFS fallback as `metadata.db`
- API calls happen on the server (Pi), which needs internet regardless of where storage is

### Rate Limiting

- If RA returns a rate limit error (HTTP 429 or similar), back off and show cached data
- Log the rate limit event for debugging
- The `GetGameList` per-system call is the heaviest -- only run during explicit sync, never on page load

### ROM Matching Ambiguity

- Multiple RA games may match a single title (e.g., "Sonic the Hedgehog" exists for Mega Drive, Master System, and Game Gear)
- Always scope title matching to the correct RA console ID (derived from the RePlayOS system folder)
- If multiple games match within the same system (rare, but possible with subsets/hacks), prefer the one with more players (`NumDistinctPlayers`)

### Arcade ROM Matching

- Arcade ROMs are zip files named by MAME/FBNeo shortnames (e.g., `sf2.zip`)
- RA Arcade games may have different naming -- match on display name from our arcade DB
- RA ConsoleID 27 ("Arcade") covers FBNeo -- MAME 2003+ and MAME current may or may not be the same console ID
- Arcade matching will likely need special handling; prioritize non-arcade systems in Phase 1

### Large Libraries

- A user with 1000+ ROMs across multiple systems
- Lazy per-game fetching means only visited games generate API calls — a library of 1000 ROMs with 200 unique visits generates 200 calls (cached for 1 hour each)
- The RA game index sync (`GetGameList` per system) is the heaviest operation — runs once on setup, then weekly. For systems with thousands of games (e.g., NES), cache aggressively
- No batch progress sync by default — avoids rate limit issues with large libraries

---

## 9. File Structure

```
replay-control-core/src/
├── retroachievements/           # New module
│   ├── mod.rs                   # Public API, feature gate
│   ├── client.rs                # HTTP client wrapper for RA API
│   ├── types.rs                 # RA API response types (deserialization)
│   ├── db.rs                    # SQLite operations for RA tables
│   ├── matching.rs              # ROM-to-RA game matching logic
│   └── system_map.rs            # Static RePlayOS folder → RA console ID mapping

replay-control-app/src/
├── server_fns/
│   └── retroachievements.rs     # Server functions for RA features
├── pages/
│   └── retroachievements.rs     # Settings/profile page for RA
├── components/
│   ├── achievement_list.rs      # Achievement list component (game detail)
│   ├── achievement_badge.rs     # Small badge/progress indicator (game list)
│   └── achievement_summary.rs   # Stats summary (home page)
```

Feature-gated behind `retroachievements` in the core crate's `Cargo.toml` (similar to `metadata` feature), so it can be compiled out if not needed.

---

## 10. Dependencies

- **`hyper` + `hyper-rustls` + `hyper-util`** -- HTTP client (already transitive dependencies via axum/leptos, so near-zero added compile cost)
- **`http-body-util`** -- body utilities for hyper (already in dependency tree)
- **`md5`** -- for ROM hashing in Phase 3 (small, no-dependency crate)
- No RA client library exists for Rust -- we call the REST API directly, which is simpler and avoids an unnecessary dependency
- **NOT using `reqwest`** -- would add ~30 transitive crates and increase compile time significantly, for no benefit given our simple GET-only API usage

---

## 11. References

- [RA Web API documentation](https://api-docs.retroachievements.org/)
- [RA API docs GitHub repo](https://github.com/RetroAchievements/api-docs)
- [RAWeb source (PHP)](https://github.com/RetroAchievements/RAWeb)
- [rcheevos (emulator integration library)](https://github.com/RetroAchievements/rcheevos) -- NOT needed for our use case
- [RA Discord `#coders` channel](https://discord.gg/dq2E4hE) -- for rate limit discussions
