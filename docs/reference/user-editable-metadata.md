# User-Editable Game Metadata

Design for allowing users to manually edit game metadata (descriptions, ratings, publisher) on a per-game basis.

**Status:** Nice-to-have / Future
**Depends on:** Game metadata system (Phase 1 — implemented)

---

## 1. Motivation

The LaunchBox import matches ~55-80% of games depending on the system. For unmatched games, or games with inaccurate/missing data, users should be able to:

- Write their own descriptions
- Set their own ratings
- Correct publisher/developer info
- Override imported data they disagree with

User edits should be durable — they must survive re-imports, metadata clears, and storage changes.

---

## 2. Data Model

### Separate table for user overrides

A new `user_metadata` table stores only fields the user has explicitly changed. This keeps user data completely independent from imported data.

```sql
CREATE TABLE IF NOT EXISTS user_metadata (
    system TEXT NOT NULL,
    rom_filename TEXT NOT NULL,
    description TEXT,       -- NULL = no override, "" = explicitly cleared
    rating REAL,            -- NULL = no override
    publisher TEXT,          -- NULL = no override, "" = explicitly cleared
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (system, rom_filename)
);
```

### Why not modify the existing table?

The current `game_metadata` table has `PRIMARY KEY (system, rom_filename)` with a `source` column. We considered three alternatives:

| Approach | Pros | Cons |
|----------|------|------|
| **A. Extra columns** (`user_description`, etc.) | Simple, single row | Schema grows per field; semantically muddy |
| **B. Separate table** | Clean separation; user data survives clears/re-imports | Join on lookup |
| **C. Multi-row** (PK includes source) | Supports N sources | Migration; complex priority resolution |

**Choice: B (separate table).** The join cost is negligible (two indexed lookups), and the separation guarantees:

- `CLEAR ALL METADATA` only clears imported data, never user edits
- Re-importing LaunchBox XML never touches user overrides
- User can "reset to imported" by deleting their row

### Per-field merge semantics

When building `GameMetadata` for a game:

1. Load imported row from `game_metadata` (may be `None`)
2. Load user row from `user_metadata` (may be `None`)
3. For each field, prefer user value if non-NULL:

```
final.description = user.description.or(imported.description)
final.rating      = user.rating.or(imported.rating)
final.publisher   = user.publisher.or(imported.publisher)
```

**NULL vs empty string:**
- `NULL` in `user_metadata` = "no override, use imported value"
- `""` (empty string) = "user explicitly cleared this field"
- Non-empty string = "user's custom value"

This allows the user to remove an imported description without deleting the entire row.

---

## 3. Core API

### New types

```rust
/// User-provided metadata for a single game.
pub struct UserMetadata {
    pub description: Option<String>,
    pub rating: Option<f64>,
    pub publisher: Option<String>,
    pub updated_at: i64,
}
```

### New MetadataDb methods

```rust
impl MetadataDb {
    /// Create the user_metadata table if it doesn't exist.
    /// Called from init() alongside the existing table creation.
    fn init_user_table(&self) -> Result<()>;

    /// Look up user overrides for a game.
    pub fn lookup_user(&self, system: &str, rom_filename: &str) -> Result<Option<UserMetadata>>;

    /// Save user metadata (upsert). Only non-None fields are written.
    pub fn save_user_metadata(
        &self,
        system: &str,
        rom_filename: &str,
        description: Option<String>,
        rating: Option<f64>,
        publisher: Option<String>,
    ) -> Result<()>;

    /// Remove all user overrides for a game (reset to imported data).
    pub fn reset_user_metadata(&self, system: &str, rom_filename: &str) -> Result<()>;

    /// Modify lookup() to merge user overrides.
    /// Returns (merged_metadata, has_user_edits).
    pub fn lookup_merged(
        &self,
        system: &str,
        rom_filename: &str,
    ) -> Result<(Option<GameMetadata>, bool)>;
}
```

### Modified lookup flow

The existing `enrich_from_metadata_cache()` in `server_fns.rs` calls `db.lookup()`. After this change, it would call `db.lookup_merged()` which:

1. Queries `game_metadata` for imported data
2. Queries `user_metadata` for user overrides
3. Merges user fields over imported fields
4. Returns a flag indicating whether user edits exist

### New server functions

```rust
/// Save user metadata for a game.
#[server(prefix = "/sfn")]
pub async fn save_game_metadata(
    system: String,
    rom_filename: String,
    description: Option<String>,
    rating: Option<f64>,
    publisher: Option<String>,
) -> Result<(), ServerFnError>;

/// Reset user metadata for a game (revert to imported data).
#[server(prefix = "/sfn")]
pub async fn reset_game_metadata(
    system: String,
    rom_filename: String,
) -> Result<(), ServerFnError>;
```

### GameInfo changes

Add a flag to `GameInfo` so the UI knows whether the current data includes user edits:

```rust
pub struct GameInfo {
    // ... existing fields ...

    /// True if the user has manually edited any metadata for this game.
    pub has_user_edits: bool,
}
```

---

## 4. UI Design

### Game detail page — inline editing

The game detail page gets an "Edit Metadata" button. When active, the description, rating, and publisher fields become editable.

**Layout (view mode — current):**

```
[Game Info section]
  System:      Super Nintendo
  Developer:   Nintendo
  Publisher:   Nintendo           <-- from imported or user data
  Rating:      4.2 / 5.0         <-- from imported or user data
  ...

[Description section]
  The Legend of Zelda: A Link to the Past is an action-adventure...
                                          [Edit Metadata]  ← button
```

**Layout (edit mode):**

```
[Description section]
  ┌─────────────────────────────────────────────┐
  │ The Legend of Zelda: A Link to the Past is  │
  │ an action-adventure...                      │  ← textarea
  │                                             │
  └─────────────────────────────────────────────┘

  Rating:    [★★★★☆]  or  [4.2  ]  ← number input

  Publisher: [Nintendo_________]    ← text input

  [Save]  [Cancel]  [Reset to imported]
```

### Interaction details

1. **Edit button**: Appears in the Description section, next to the section title. Only shows if metadata DB is available.

2. **Edit mode**:
   - Description becomes a `<textarea>` pre-filled with current value (user or imported)
   - Rating becomes a number input (0.0 - 5.0, step 0.1)
   - Publisher becomes a text input
   - Three buttons: Save, Cancel, Reset

3. **Save**: Calls `save_game_metadata`. On success, exits edit mode and refetches the detail resource.

4. **Cancel**: Discards changes, exits edit mode.

5. **Reset to imported**: Calls `reset_game_metadata`. Removes all user overrides; the game reverts to imported data (or no data if never imported). Requires confirmation.

6. **Visual indicator**: When a game has user edits, show a small badge or indicator (e.g., pencil icon) next to the edited fields so the user knows the data is their own.

### Component structure

```rust
#[component]
fn MetadataEditor(
    system: StoredValue<String>,
    rom_filename: StoredValue<String>,
    description: Option<String>,
    rating: Option<f32>,
    publisher: Option<String>,
    has_user_edits: bool,
    on_saved: Callback<()>,
) -> impl IntoView;
```

### Mobile considerations

- The textarea should be full-width and at least 4 rows tall
- Rating input should be large enough for touch (use star buttons or a slider)
- Save/Cancel/Reset buttons should be full-width stacked on mobile

---

## 5. Edge Cases

### Game with no imported metadata

User can still add their own description/rating/publisher. The `user_metadata` table doesn't require a corresponding `game_metadata` row.

### Re-import after user edits

User edits are in a separate table, so re-importing LaunchBox XML only affects `game_metadata`. The merged lookup still prefers user values.

### Clear all metadata

"Clear All Metadata" on the metadata page only clears `game_metadata` (imported data). User edits in `user_metadata` are preserved. A separate "Clear User Edits" option could be added but is lower priority.

### Rating scale

LaunchBox uses a 0.0-5.0 community rating. User ratings should use the same scale for consistency. The UI can show this as stars (1-5) or a numeric input.

### Empty vs unset

When the user saves with an empty description, store `""` (empty string) in `user_metadata.description`. This means "user explicitly wants no description" and will show "No description available" even if imported data has one.

To remove an override for a single field without resetting all fields, the user would need per-field reset buttons (future enhancement). Initially, "Reset to imported" clears all user overrides for the game.

---

## 6. Implementation Plan

### Step 1: Database layer
- Add `user_metadata` table creation in `MetadataDb::init()`
- Add `lookup_user()`, `save_user_metadata()`, `reset_user_metadata()` methods
- Modify `lookup()` to merge user overrides (or add `lookup_merged()`)

### Step 2: Server functions
- Add `save_game_metadata` and `reset_game_metadata` server functions
- Register in `main.rs`
- Add `has_user_edits` field to `GameInfo` and populate in `enrich_from_metadata_cache()`

### Step 3: UI — edit mode
- Add `MetadataEditor` component
- Wire up description textarea, rating input, publisher input
- Save/Cancel/Reset handlers
- Refetch game detail on save

### Step 4: Visual indicators
- Show pencil icon or "edited" badge on user-modified fields
- Add "Reset to imported" confirmation dialog

### Estimated effort: ~1 week
