# RetroAchievements data

This directory holds per-system RetroAchievements title lists that the catalog
build (`tools/build-catalog`) ingests into `canonical_game.retroachievements_id`.

## File format

One JSON file per system, named after the **system name** (the same slug used
elsewhere in the catalog, e.g. `nintendo_snes.json`, `sega_smd.json`). Each file
is an array of objects:

```json
[
  { "title": "Super Mario World", "ra_id": "228" },
  { "title": "The Legend of Zelda: A Link to the Past", "ra_id": "1234" }
]
```

- `title` — the game's display title. Matching is done with the same
  normalization the runtime scanner uses
  (`title_utils::normalize_title_for_metadata`), so regional tags, articles, and
  punctuation differences are tolerated.
- `ra_id` — the RetroAchievements game id (kept as a string).

The extract also records `num_achievements`, which the build ignores; only
`title` + `ra_id` are read.

## Scope

Only the systems the catalog builds `canonical_game` metadata for are present —
the same set `scripts/download-metadata.sh` fetches No-Intro DATs for. Disc
systems (PlayStation, PS2, DS, Saturn, Dreamcast, …), Atari, NEC, SNK, and 3DO
are not in the catalog, so RA data for them could never be ingested and is not
fetched. Arcade is deferred: RA's "Arcade" console is keyed by RA display titles
(hack-heavy) while `arcade_game` rows are MAME romset names, so title-matching is
unreliable and needs a separate rom-name/hash approach.

## Where the data comes from

These files **are committed**, mirroring the wikidata/shmups committed-data
pattern, so release and CI catalog builds get RA ids without needing an API key.
They are refreshed by `scripts/retroachievements-gamelist-extract.py`, which
queries the RetroAchievements API (requires a read-only Web API key):

```sh
RA_API_KEY=<web-api-key> python3 scripts/retroachievements-gamelist-extract.py
```

When no files are present, the build does nothing and the `retroachievements_id`
column stays empty — the column, search filter, and detail pill all degrade
cleanly to "no RA support known."
