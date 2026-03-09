# Skin/Theming Analysis

Research on syncing Replay Control's appearance with ReplayOS's active skin.

## 1. ReplayOS Skin System

### How it works

The `system_skin` key in `replay.cfg` holds an integer index (e.g. `system_skin = "3"`).
The ReplayOS frontend (`/opt/replay/replay`) loads three PNG images per skin from
`./images/` (relative to the binary, so `/opt/replay/images/`):

| Image             | Size    | Purpose                                       |
|-------------------|---------|-----------------------------------------------|
| `menu_{N}.png`    | 192x192 | Menu background — tiled/stretched behind the UI |
| `selector_{N}.png`| 192x9   | Selection highlight bar color                 |
| `info_{N}.png`    | 192x16  | Info/status bar background strip              |

Skins 0-10 are built-in with distinct color palettes. Skins 11-17 are user-customizable
slots (7 slots; the changelog mentions "26 extra slots" which likely counts all three
image types per slot, not yet all populated). Custom slots default to a copy of the
blue theme until the user replaces the PNGs.

There is **no text-based color definition file** — skins are purely image-based.
Colors must be **extracted from the PNGs** at runtime.

### Built-in skin color palettes (extracted from images)

| Skin | Menu BG          | Accent/Header    | Selector         | Character             |
|------|------------------|------------------|------------------|-----------------------|
| 0    | #101b32 (navy)   | #065ab5 (blue)   | #be1250 (pink)   | Default ReplayOS blue |
| 1    | #2d312d (carbon) | #848684 (grey)   | #ff004a (hot pink)| Dark carbon           |
| 2    | #005100 (green)  | #005162 (teal)   | #ff4300 (orange) | Green terminal        |
| 3    | #000000 (black)  | #00b543 (green)  | #a2426e (mauve)  | Matrix green/black    |
| 4    | #000000 (black)  | #2f54a4 (blue)   | #8b2123 (red)    | RGB retro             |
| 5    | #0f0f0f (dark)   | #ff0000 (red)    | #ff0000 (red)    | Red/dark              |
| 6    | #4e4a4e (grey)   | #854c30 (brown)  | #6daa2c (green)  | Pixel-art leather     |
| 7    | #070572 (indigo) | #e4eaf5 (white)  | #be1250 (pink)   | Deep blue             |
| 8    | #171717 (dark)   | #ffffff (white)   | #4c007f (purple) | Minimal dark          |
| 9    | #000000 (black)  | #777777 (grey)   | #7e2553 (plum)   | Noir chrome           |
| 10   | #000000 (black)  | #a09460 (gold)   | #505050 (grey)   | Gold/black            |

### What color data is extractable

From each skin's PNGs we can reliably extract:

- **Background color**: dominant color from `menu_{N}.png` (most frequent pixel)
- **Accent/header color**: second or third most frequent color from `menu_{N}.png`
- **Selection/highlight color**: dominant non-transparent color from `selector_{N}.png`
- **Info bar color**: dominant color from `info_{N}.png` (usually matches background)

We cannot extract: text colors, font info, border colors, or hover states — those
don't exist in the skin system. We must derive them from the extracted colors.

## 2. Replay Control Theme Mapping

### Current CSS variables (from `:root` in style.css)

```
--bg, --surface, --surface-hover, --border,
--text, --text-secondary, --accent, --accent-hover,
--star, --error, --success, --radius, --radius-sm
```

### Proposed mapping: ReplayOS skin -> CSS variables

| CSS Variable       | Source                                              |
|--------------------|-----------------------------------------------------|
| `--bg`             | Dominant color from `menu_{N}.png`                  |
| `--surface`        | `--bg` lightened 8-10% (or darkened if light theme) |
| `--surface-hover`  | `--surface` lightened 5%                            |
| `--border`         | `--bg` lightened 15%                                |
| `--text`           | Auto: white (#e4e6ea) if bg is dark, dark (#1a1d23) if bg is light |
| `--text-secondary` | `--text` at 55% opacity                             |
| `--accent`         | Selection color from `selector_{N}.png`             |
| `--accent-hover`   | `--accent` lightened 15%                            |
| `--star`           | Keep fixed (#f59e0b) or tint toward accent          |
| `--error`          | Keep fixed (#ef4444)                                |
| `--success`        | Keep fixed (#22c55e)                                |

### Fallback strategy

If skin images can't be read (e.g., rootfs not mounted, SD card not present during
development), fall back to the current hardcoded dark theme. The app already works
without a live SD card via `--storage-path`.

## 3. Implementation Approach

### Reading the active skin

1. `ReplayConfig` already parses `replay.cfg` and can read `system_skin`.
2. Add a `system_skin()` accessor to `ReplayConfig` returning the index (default: `0`).
3. At startup (and on config change), read `menu_{N}.png`, `selector_{N}.png` from
   the rootfs. The images live at `/opt/replay/images/` on the Pi. When developing
   off-device, accept a `--skin-images-path` CLI flag or skip theming gracefully.
4. Extract dominant colors using a simple pixel-frequency analysis (no external crate
   needed — just decode the PNG and count pixel values with the `image` crate already
   available in the Rust ecosystem).

### Serving dynamic CSS variables

**Recommended: inline `<style>` block in the `<head>`**, generated server-side.

```html
<style id="skin-theme">
  :root {
    --bg: #101b32;
    --surface: #1a2340;
    /* ... derived values ... */
  }
</style>
```

This approach:
- Works with SSR (no flash of unstyled content)
- Doesn't require a separate CSS endpoint
- Overrides the defaults in `style.css` via cascade order
- The `Shell` component already controls `<head>` and can inject this

The static `style.css` keeps its current `:root` values as defaults. The inline
`<style>` block overrides only the color variables when a skin is active.

**Alternative considered and rejected**: dynamic `/theme.css` endpoint. This would
require cache busting, adds a blocking request, and is harder to keep in sync with SSR.

### Config file for Replay Control preferences

Per `features.md`, Replay Control should NOT write to `replay.cfg`. A separate
`replay-companion.cfg` (same key=value format, parsed by `ReplayConfig`) would store:

```
theme_sync = "true"          # sync with ReplayOS skin (default: true)
theme_override = "dark"      # manual override: "dark", "light", or skin index
```

Location: alongside `replay.cfg` in the config directory.

### File watcher integration

The existing `spawn_storage_watcher` already watches `replay.cfg` with inotify and
has debouncing. When `system_skin` changes:

1. The watcher detects the config file change (already implemented).
2. `refresh_storage()` re-reads the config (already implemented).
3. Add: after refresh, extract the new skin's colors and update a `SkinTheme` in
   `AppState` (behind an `RwLock`, similar to `storage`).
4. The next SSR render picks up the new theme automatically.
5. For already-connected clients: either rely on next navigation (SSR re-render)
   or add an SSE/polling endpoint for live theme updates (lower priority).

### SSR considerations

The theme **must** be applied in the `Shell` component during SSR to avoid a flash
of wrong colors. Since `AppState` is available via `provide_context`, the Shell can
read the current skin theme and inject the inline `<style>`:

```rust
// In Shell component
let theme_css = expect_context::<AppState>().skin_theme_css();
// Then in view: <style>{theme_css}</style>
```

The `meta[name="theme-color"]` should also update to match `--bg` so the mobile
browser chrome matches.

## 4. UI Changes Required

### CSS changes

- **No structural CSS changes needed.** All color usage already goes through CSS
  variables. The entire theme change is just overriding `:root` variable values.
- Remove the hardcoded `rgba()` values in `.status-ok` and `.status-err` — replace
  with `color-mix()` or CSS variables for the semi-transparent backgrounds.
- The SVG dropdown arrow in `select.form-input` has a hardcoded fill color (`%238b8f96`)
  that should become a CSS variable or be generated dynamically.

### Contrast and accessibility

- Auto-derive `--text` based on background luminance (WCAG contrast ratio >= 4.5:1).
- Some skins have very low contrast between menu BG and accent (e.g., skin 9 is
  all black/grey). Need a minimum contrast check; if accent is too close to BG,
  lighten/saturate the accent.
- Consider clamping derived surface/border colors to ensure they're always
  distinguishable from the background.

### "Disable theming" toggle

- A toggle in the More/Settings page: "Sync theme with ReplayOS skin".
- When disabled, the app uses the default dark theme (current hardcoded values).
- Stored in `replay-companion.cfg` as `theme_sync = "false"`.

### Dark/light mode

ReplayOS skins don't include an explicit dark/light mode flag. However, the background
luminance determines this implicitly:
- Most skins are dark (black/navy/dark grey backgrounds) -> white text.
- Skin 2 (green) and skin 7 (deep blue) are on the boundary.
- No built-in skins are truly light-themed.

Replay Control should compute luminance and choose text color accordingly, but a
full light-mode design (inverted surfaces, borders, etc.) is probably unnecessary
given the skin palette range.

## 5. Complexity Estimate

| Component                                  | Complexity | Notes                                       |
|--------------------------------------------|------------|---------------------------------------------|
| Read `system_skin` from config             | Low        | One new accessor on `ReplayConfig`          |
| PNG color extraction                       | Low        | ~50 lines, `image` crate                    |
| Color derivation (surface, border, text)   | Medium     | Luminance calc, contrast checks, HSL math   |
| Inline `<style>` injection in Shell        | Low        | String interpolation in Shell component     |
| `SkinTheme` in AppState + RwLock           | Low        | Mirror existing `storage` pattern           |
| File watcher skin refresh                  | Low        | Extend existing `refresh_storage` path      |
| Companion app config file                  | Medium     | New file, new parse path, settings UI       |
| Settings UI for theme toggle               | Low        | One checkbox on More page                   |
| Fix hardcoded colors in CSS                | Low        | 2-3 small CSS edits                         |
| Contrast/accessibility safety              | Medium     | Algorithm to ensure readable combinations   |
| Live theme update (SSE or polling)         | Medium     | Optional, can defer to Phase 2              |

### Suggested phasing

**Phase 1 — Basic skin sync (Low effort, high impact)**
- Read `system_skin`, extract colors from PNGs, derive theme palette.
- Inject inline `<style>` in Shell. Update `theme-color` meta tag.
- Falls back to default theme when images unavailable.
- No config file, no settings UI, no live updates.

**Phase 2 — User control (Medium effort)**
- `replay-companion.cfg` with `theme_sync` toggle.
- Settings UI on More page.
- Fix hardcoded CSS colors (dropdown arrow, status backgrounds).

**Phase 3 — Polish (Medium effort)**
- Live theme updates via SSE when skin changes while client connected.
- Contrast safety checks and auto-adjustment.
- Predefined theme palette map as alternative to runtime image parsing
  (hardcode the 11 built-in skin palettes, only parse PNGs for custom slots).
