# ROM Classification

Defined in `replay-control-core/src/game/rom_tags.rs`.

## RomTier Enum

ROMs are classified into tiers that determine sort order (lower = shown first) and filtering behavior:

| Tier | Value | Description |
|------|-------|-------------|
| Original | 0 | Clean original ROM |
| Revision | 1 | Revision of an original (Rev 1, Rev A) |
| RegionVariant | 2 | Non-primary region variant |
| Translation | 3 | Translation patch applied |
| Unlicensed | 4 | Unlicensed but commercial |
| Homebrew | 5 | Homebrew / aftermarket |
| Hack | 6 | ROM hack |
| PreRelease | 7 | Beta, prototype, demo, sample |
| Pirate | 8 | Pirate / bootleg |

## classify() Function

`classify(filename) -> (RomTier, RegionPriority, bool)`

Parses all parenthesized `(...)` and bracketed `[...]` tags from the filename stem (extension stripped). Sets boolean flags as tags are recognized, then determines the tier from flag priority:

```
Pirate > PreRelease > Hack > Homebrew > Unlicensed > Translation > Revision > RegionVariant > Original
```

The first matching tier wins (checked in descending severity order).

### Output fields

- **RomTier**: classification tier
- **RegionPriority**: `World > Usa > Europe > Japan > Other > Unknown` (configurable via `RegionPreference`)
- **is_special** (bool): `true` for ROMs excluded from recommendations and the regional variants chip row. Includes Unlicensed, Homebrew, PreRelease, Pirate tiers, plus FastROM, Extended Screen, and 60Hz patches.

## Recognized Parenthesized Tags

### Regions
`(USA)`, `(Europe)`, `(Japan)`, `(World)`, `(Spain)`, `(France)`, `(Germany)`, etc. Multi-region: `(USA, Europe)`. TOSEC two-letter codes: `(US)`, `(EU)`, `(JP)`, `(GB)`, `(ES)`, `(FR)`, `(DE)`.

### Revisions
`(Rev 1)`, `(Rev A)`, `(Rev 2)`, `(REV01)`, `(REV02)`.

### Translations
`(Traducido Es)`, `(Traduzido Por)`, `(Translated En)`, `(Translated Fre)`, `(PT-BR)`.

### Status markers
`(Hack)`, `(SMW Hack)`, `(SA-1 SMW Hack)`, `(Beta)`, `(Proto)`, `(Prototype)`, `(Demo)`, `(Sample)`, `(Unl)`, `(Unlicensed)`, `(Aftermarket)`, `(Homebrew)`, `(Pirate)`.

### Patches
`(60hz)`, `(FastRom)`, `(Extended Screen)`.

### Distribution channels (shown verbatim)
`(SegaNet)`, `(BS)` (Satellaview), `(Sega Channel)`, `(Sufami Turbo)`.

### Platform variants
`(Sega CD 32X)`, `(Mega-CD 32X)` -- shown as "CD 32X".

## TOSEC Bracket Flags

Bracket tags `[...]` are classified into dump quality flags:

| Flag | Pattern | Classification |
|------|---------|----------------|
| Alternate | `[a]`, `[a2]` | Revision tier |
| Hack | `[h]`, `[h Hack Name]` | Hack tier |
| Cracked | `[cr]`, `[cr Cracker]` | Hack tier |
| Trained | `[t]`, `[t +2]` | Hack tier |
| Fixed | `[f]`, `[f1]` | Revision tier |
| Overdump | `[o]`, `[o1]` | Revision tier |
| Bad Dump | `[b]`, `[b1]` | Pirate tier |
| Pirate | `[p]`, `[p1]` | Pirate tier |

`[!]` (verified good dump) is explicitly skipped.

### Bracket translations
`[T-Spa1.0v_Wave]` -> "ES Translation", `[T+Fre]` -> "FR Translation", `[T+Rus Pirate]` -> "RU Translation". The language code is extracted and normalized; hacker credits are stripped.

## Noise Tags (filtered from display)

Tags that don't help distinguish ROM versions are suppressed:
- Verified dump markers: `[!]`
- Version dates in brackets: `[2017-03-28]`
- Virtual Console / Switch Online markers
- Standalone language codes already covered by region: `(En)`, `(Ja)`, `(En,Fr,De)`
- Platform markers: `(NP)` (Nintendo Power)

## TOSEC Language Codes

All-lowercase parenthesized tags like `(fr)`, `(es)`, `(en-de)` are recognized as TOSEC language codes. They're expanded to full names for display (e.g., "French", "Spanish") and mapped to region strings for the `region` column.

## extract_tags()

Builds a display suffix string from all recognized tags:

```
"USA, Rev 1, ES Translation, 60Hz, Hack"
```

Order: region, TOSEC language, distribution channels, revision, translation, patches, status markers, TOSEC bracket flags, platform variant.

## extract_bracket_descriptors()

Returns non-standard bracket tag content (not dump flags, not translations, not dates). Used for disambiguation when multiple non-clone entries share the same display name -- e.g., `[joystick]`, `[experimental]`, `[full]`.
