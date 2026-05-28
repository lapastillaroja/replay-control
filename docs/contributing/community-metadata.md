# Contributing community metadata

Some games and ROMs are not covered by the bundled metadata sources (No-Intro, TheGamesDB, MAME, FBNeo) or by optional external sources (LaunchBox). Curated distributions like **AmigaVision**, aftermarket/homebrew releases, fan translations with their own identity, and one-off compilations all fall through these gaps.

Anyone can submit a pull request that adds metadata for these entries. The data lives in JSON files under `data/community/` and is baked into `catalog.sqlite` at build time. No Rust code changes are required to add a new game.

## File layout

One file per system. The file stem is the system folder name used internally by replay-control:

```
data/community/commodore_ami.json     ← Amiga
data/community/nintendo_snes.json     ← SNES
data/community/sony_psx.json          ← PlayStation
```

Find your system's folder name by looking at existing ROM directories on the device or grepping `replay-control-core/src/platform/systems.rs`.

## Entry schema

```json
{
  "$schema": "../../schemas/community-metadata.schema.json",
  "entries": [
    {
      "filename_stem": "AmigaVision",
      "display_name": "AmigaVision",
      "year": 2024,
      "developer": "AmigaVision Project",
      "publisher": "AmigaVision Project",
      "genre": "Compilation",
      "players": 1,
      "coop": false,
      "description": {
        "en": "AmigaVision is a curated collection of ~3,000 pre-installed Amiga games and demoscene productions."
      },
      "boxart_url": "https://...",
      "title_image_url": "https://...",
      "screenshot_urls": ["https://...", "https://..."],
      "manuals": [
        { "url": "https://.../readme.pdf", "language": "en", "title": "Readme" }
      ],
      "videos": [
        { "url": "https://www.youtube.com/watch?v=...", "title": "Overview" }
      ],
      "strategy_guides": [
        { "url": "https://...", "title": "Setup Guide" }
      ],
      "tags": ["compilation", "curated"]
    }
  ]
}
```

### Required fields

- **`filename_stem`** — the ROM filename without extension (e.g. `AmigaVision` for `AmigaVision.hdf`). This is the key replay-control's scan uses to match on disk.
- **`display_name`** — what the UI shows.

### Optional fields

- **`year`**, **`developer`**, **`publisher`**, **`genre`**, **`players`**, **`coop`** — populate whatever you have. Empty defaults are fine.
- **`description`** — long-form text. Either a bare string (treated as English) or `{ "en": "...", "ja": "...", "es": "..." }`. The `en` key is required when using the polyglot form.
- **`boxart_url`**, **`title_image_url`**, **`screenshot_urls[]`** — image URLs. *(See "Image download status" below.)*
- **`manuals[]`** — PDF/HTML manuals, each `{ url, title?, language?, mime_type? }`. Surfaces in the existing manuals UI.
- **`videos[]`** — per-game video recommendations, each `{ url, title? }`. Surfaces in the existing video panel.
- **`strategy_guides[]`**, **`video_indexes[]`** — external deep links, each `{ url, title? }`.
- **`tags[]`** — free-form contributor notes (not surfaced in the UI today).
- **`crc32`** — optional 8-hex-character secondary match key, useful for cartridge-system aftermarket releases where filenames vary.
- **`override`** — set to `true` to deliberately replace an existing entry from another source. The build aborts otherwise.

## Image URL policy

URLs can point anywhere today. Reviewers will judge link stability case by case in the PR. Goal is to migrate toward a stable-host requirement (a companion media repo or `raw.githubusercontent.com` URLs) — until then, please prefer hosts that are unlikely to disappear. **Avoid** image sharers, personal blogs, and tracker-heavy CDNs.

### Image download status

Image URLs (`boxart_url`, `title_image_url`, `screenshot_urls`) are written into `catalog.sqlite` today. The runtime download path that pulls them into the per-storage media folder is a follow-up — for now, the data is in the catalog and contributors can submit it, but the game detail page does not yet render community images. Manuals, videos, strategy guides, and video indexes surface immediately in the existing UIs.

Multi-screenshot rendering on the detail page is a separate follow-up. The schema accepts multiple URLs today; only the first one will be picked up once the download path lands.

## Licensing

By submitting a PR you agree that contributed text is your own or under a license compatible with this project's GPLv3 (CC0 preferred for descriptions). Image URLs must point at content the original host has the right to serve. Do not link directly to copyrighted scans or screenshots from publisher sites without permission.

## Collisions

If your `(system, filename_stem)` already has an entry from No-Intro, TheGamesDB, or another community PR, the catalog build will fail with a clear error naming the existing source. Add `"override": true` to the entry only when you are deliberately replacing the existing metadata — for example, the upstream entry has the wrong year and the community entry corrects it. Don't override casually; the source attribution stays on `canonical_game.source` for review.

## Submitting

1. Edit or create `data/community/<system>.json`.
2. Validate the file against `schemas/community-metadata.schema.json` (your editor likely does this automatically when the `$schema` line is present).
3. Run the catalog build locally to confirm:
   ```sh
   cargo run --release -p build-catalog -- --output catalog.sqlite
   ```
   Look for `Community metadata: Inserted N entries`.
4. Run the unit tests:
   ```sh
   cargo test -p build-catalog community
   cargo test -p replay-control-core community
   ```
5. Open a PR with a `feat(community):` Conventional Commit title. Mention the system, the entry, and a one-line rationale.
