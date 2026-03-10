# Games Tab Analysis

## Decision: Option B — Merge Games into Home

Decided 2026-03-11. The Games tab is removed; Home becomes the browsing hub.

## Context

With global search now supporting platform filtering, genre browsing, and cross-system results, the dedicated "Games" tab is redundant.

## What Changes

### Bottom Navigation: 4 tabs → 3
```
Before: [Home] [Games] [Favorites] [More]
After:  [Home] [Favorites] [More]
```

### Home Page: New Layout
1. Search bar
2. Last Played (hero card)
3. Recently Played (horizontal scroll)
4. Library stats (4 cards — Games, Systems, Favorites, Storage)
5. All Systems grid (greyed-out if empty, not clickable)

### Decisions

| Question | Answer |
|----------|--------|
| Empty systems clickable? | **No** — inert, greyed-out. Future: system info pages could make them clickable. |
| USED + STORAGE stats? | **Merge** into single card (e.g., "256GB / 1TB USB") → 4 stats, fits 2x2 on mobile |
| Systems before or after stats? | **After stats** — stats are a quick glance, systems are the browsing grid |

### Routes
- `/games` — **removed** (redirect to `/` if hit directly)
- `/games/:system` — **kept** (ROM list page, deep linking)
- `/games/:system/:filename` — **kept** (game detail page)

## Implementation

| File | Change |
|------|--------|
| `pages/home.rs` | Show all systems (unfiltered), merge USED+STORAGE stat cards |
| `components/nav.rs` | Remove Games tab |
| `lib.rs` | Remove `/games` route (keep sub-routes) |
| `pages/games.rs` | Remove `GamesPage` (keep `SystemRomView`) |
| `style/style.css` | Minor spacing tweaks |
| `i18n.rs` | Update/remove Games-specific keys |

## Future Ideas

- **System info pages**: Each system gets a dedicated page with history,
  emulator details, compatibility notes, etc. The greyed-out empty system cards
  on Home would become natural entry points ("you don't have games for this
  system, but here's what it is"). Route: `/systems/:system`.

## Previous Analysis

### What Games provided that Search doesn't
1. System overview dashboard — all systems at once with metadata (manufacturer, size)
2. Full system browsing — all ROMs in a system, alphabetical, paginated
3. System-level context — focused view for exploring a system

### Why merge works
- System overview moves to Home (all systems grid)
- Full system browsing stays via `/games/:system`
- Search handles cross-system discovery
- Redundant system grids eliminated
