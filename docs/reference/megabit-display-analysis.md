# Megabit Display Analysis

How to show ROM sizes using historically accurate units -- Megabit (Mbit) for cartridge games, Megabytes (MB/GB) for disc and computer games.

---

## 1. System Classification

### Cartridge-based systems (use Megabit)

These systems used ROM chips measured in Megabits on their original packaging and marketing:

| System (folder_name) | Display Name | Typical ROM sizes | Notes |
|---|---|---|---|
| `nintendo_nes` | Nintendo Entertainment System | 128 Kbit -- 4 Mbit | "PRG-ROM: 256 Kbit, CHR-ROM: 128 Kbit" on labels |
| `nintendo_snes` | Super Nintendo | 2 -- 48 Mbit | "8 Mbit" (SMW), "32 Mbit" (DKC2), "48 Mbit" (Tales of Phantasia) |
| `nintendo_n64` | Nintendo 64 | 32 -- 512 Mbit | "64 Mbit" (SM64), "256 Mbit" (RE2), "512 Mbit" (Conker) |
| `nintendo_gb` | Game Boy | 256 Kbit -- 8 Mbit | "4 Mbit" on Pokemon Red |
| `nintendo_gbc` | Game Boy Color | 256 Kbit -- 16 Mbit | Same measurement tradition as GB |
| `nintendo_gba` | Game Boy Advance | 4 -- 256 Mbit | "64 Mbit" was standard; "256 Mbit" for largest titles |
| `sega_smd` | Sega Mega Drive / Genesis | 4 -- 40 Mbit | "16 Mbit" (Sonic 3), "24 Mbit" (Phantasy Star IV) |
| `sega_sms` | Sega Master System | 128 Kbit -- 4 Mbit | Cartridge ROM chips |
| `sega_sg` | Sega SG-1000 | 8 -- 256 Kbit | Very small cartridges |
| `sega_gg` | Sega Game Gear | 256 Kbit -- 4 Mbit | Same tech as SMS |
| `sega_32x` | Sega 32X | 8 -- 32 Mbit | Cart add-on for Mega Drive |
| `nec_pce` | PC Engine / TurboGrafx-16 | 2 -- 20 Mbit | HuCards were cartridge-format |
| `atari_2600` | Atari 2600 | 2 -- 64 Kbit | Extremely small ROMs |
| `atari_5200` | Atari 5200 | 8 -- 128 Kbit | Similar era to 2600 |
| `atari_7800` | Atari 7800 | 16 -- 1024 Kbit | Cartridge system |
| `atari_jaguar` | Atari Jaguar | 8 -- 48 Mbit | Cartridge system |
| `atari_lynx` | Atari Lynx | 1 -- 4 Mbit | Cartridge handheld |
| `snk_ng` | Neo Geo | 8 -- 688 Mbit | Massive cartridges; "330 Mega" on KOF labels |
| `snk_ngp` | Neo Geo Pocket | 4 -- 16 Mbit | Cartridge handheld |
| `nintendo_ds` | Nintendo DS | 8 -- 2048 Mbit | Cart-based but file sizes are typically discussed in MB. "128 MB" game cards existed. Edge case -- see section 5. |
| `microsoft_msx` | MSX | 128 Kbit -- 4 Mbit | Cartridge ROMs (`.rom`), though also has floppy (`.dsk`) |

### Disc-based systems (use MB/GB)

These systems used optical media; sizes were always discussed in Megabytes or Gigabytes:

| System (folder_name) | Display Name | Typical ROM sizes | Notes |
|---|---|---|---|
| `sony_psx` | PlayStation | 300 -- 700 MB | CD-ROM (650 MB capacity) |
| `sega_dc` | Sega Dreamcast | 200 -- 1100 MB | GD-ROM |
| `sega_cd` | Sega CD / Mega-CD | 50 -- 650 MB | CD-ROM |
| `sega_st` | Sega Saturn | 300 -- 700 MB | CD-ROM |
| `nec_pcecd` | PC Engine CD | 50 -- 650 MB | CD-ROM |
| `panasonic_3do` | 3DO | 200 -- 650 MB | CD-ROM |
| `philips_cdi` | Philips CD-i | 200 -- 650 MB | CD-ROM |
| `snk_ngcd` | Neo Geo CD | 50 -- 650 MB | CD-ROM |
| `commodore_amicd` | Commodore Amiga CD | 200 -- 700 MB | CD-ROM |

### Computer / floppy-based systems (use KB/MB)

These systems used floppy disks or disk images; sizes in KB/MB:

| System (folder_name) | Display Name | Typical ROM sizes | Notes |
|---|---|---|---|
| `commodore_ami` | Commodore Amiga | 880 KB per floppy (.adf) | ADF = 880 KB floppy image |
| `commodore_c64` | Commodore 64 | 10 -- 200 KB | Tape/disk/cartridge images |
| `ibm_pc` | IBM PC (DOS) | varies widely | Floppy/HDD images |
| `amstrad_cpc` | Amstrad CPC | 190 KB per disk (.dsk) | Floppy images |
| `sinclair_zx` | ZX Spectrum | 10 -- 128 KB | Tape/snapshot images |
| `sharp_x68k` | Sharp X68000 | 1.2 MB per floppy (.dim) | Floppy images |
| `scummvm` | ScummVM | varies widely | Point-and-click game data |

### Arcade systems (use Mbit for FBNeo/MAME, MB for Atomiswave/Naomi)

Arcade boards used ROM chips, and board specs were stated in Megabits. Classic arcade game sizes are meaningful in Mbit:

| System (folder_name) | Display Name | Recommendation | Notes |
|---|---|---|---|
| `arcade_fbneo` | Arcade (FBNeo) | Mbit | ZIP files containing ROM chip dumps. Total ROM size in Mbit matches original board specs. "CPS2 boards: 160 Mbit" etc. |
| `arcade_mame` | Arcade (MAME) | Mbit | Same rationale as FBNeo |
| `arcade_mame_2k3p` | Arcade (MAME 2003+) | Mbit | Same rationale |
| `arcade_dc` | Arcade (Atomiswave/Naomi) | MB | These used GD-ROM or flash storage. Sizes are in the hundreds of MB. |

### Utility systems (use MB/GB)

| System (folder_name) | Display Name | Notes |
|---|---|---|
| `alpha_player` | Alpha Player | Media files -- use MB/GB (currently hidden anyway) |

---

## 2. Conversion Reference

```
1 Megabit (Mbit) = 128 KB = 131,072 bytes
1 Megabyte (MB)  = 8 Megabits (Mbit) = 1,048,576 bytes

Examples:
  2 MB ROM  = 16 Mbit   (e.g. Sonic the Hedgehog on Mega Drive is ~8 Mbit)
  4 MB ROM  = 32 Mbit   (e.g. Super Mario World on SNES is ~8 Mbit)
  8 MB ROM  = 64 Mbit   (e.g. Super Mario 64 on N64 is ~64 Mbit)
  32 MB ROM = 256 Mbit   (e.g. Resident Evil 2 on N64)
  64 MB ROM = 512 Mbit   (e.g. Conker's Bad Fur Day on N64)
```

---

## 3. Current Implementation

### Where file sizes are displayed

| Location | Function used | Context |
|---|---|---|
| ROM list (per-game row) | `format_size()` | `rom_list.rs:476` -- shows e.g. "2.0 MB" |
| Game detail page (info grid) | `format_size()` | `game_detail.rs:55` -- shows e.g. "2.0 MB" |
| System card (home page) | `format_size()` | `system_card.rs:12` -- total size per system |
| Home page storage bar | `format_size_short()` | `home.rs:96-97` -- disk usage stats |
| More page (disk info) | `format_size()` | `more.rs:38-40` -- disk total/used/available |
| Metadata page | `format_size()` | `metadata.rs:96,343` -- DB size, media size |
| Search results | (none) | Search results do not show file sizes |

### Current formatting functions (`util.rs`)

- `format_size(bytes) -> String`: Returns "X KB", "X.X MB", or "X.X GB"
- `format_size_short(bytes) -> (String, &str)`: Returns `("12", "GB")` tuples with rounded GB values

### Data flow

The system identifier is available at every display point:
- **ROM list**: `rom.game.system` (the `system` folder_name string)
- **Game detail**: `detail.game.system` (same)
- **System card**: `system.folder_name` (same)

This means the formatting function can receive the system identifier and look up whether to use Mbit.

---

## 4. Proposed Implementation

### 4.1 Add media type to `System`

Add a `MediaType` enum and a field to `System` in `systems.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum MediaType {
    /// Cartridge / ROM chip -- sizes displayed in Megabit (Mbit)
    Cartridge,
    /// Optical disc -- sizes displayed in MB/GB
    Disc,
    /// Floppy disk, tape, or other computer media -- sizes in KB/MB
    Disk,
    /// Arcade ROM chips -- sizes in Mbit
    ArcadeRom,
    /// Arcade with disc-based storage (Naomi GD-ROM, etc.) -- sizes in MB
    ArcadeDisc,
}
```

Alternatively, a simpler boolean approach:

```rust
impl System {
    /// Whether this system's ROMs should be displayed in Megabit units.
    pub fn uses_megabit(&self) -> bool {
        // ...match on folder_name...
    }
}
```

**Recommendation:** Use a `uses_megabit() -> bool` method rather than adding a field. This avoids changing the `System` struct (which is `&'static`) and keeps the logic centralized. A `MediaType` enum is more expressive but not needed until other media-type-dependent behaviors emerge.

### 4.2 New formatting function

Add to `util.rs`:

```rust
/// Format a byte count using historically appropriate units for the system.
///
/// Cartridge-based systems display in Megabit (Mbit) or Kilobit (Kbit).
/// Disc/computer systems display in KB/MB/GB as before.
pub fn format_size_for_system(bytes: u64, system: &str) -> String {
    if uses_megabit(system) {
        format_size_megabit(bytes)
    } else {
        format_size(bytes)
    }
}

/// Format bytes as Megabit/Kilobit for cartridge-based systems.
fn format_size_megabit(bytes: u64) -> String {
    let bits = bytes * 8;
    const MEGABIT: u64 = 1_048_576; // 1,048,576 bits = 1 Mbit

    if bits >= MEGABIT {
        let mbit = bits as f64 / MEGABIT as f64;
        if mbit == mbit.round() {
            format!("{} Mbit", mbit as u64)
        } else {
            format!("{:.1} Mbit", mbit)
        }
    } else {
        let kbit = bits / 1024;
        if kbit > 0 {
            format!("{} Kbit", kbit)
        } else {
            format!("{} bytes", bytes)
        }
    }
}
```

### 4.3 Call sites

Replace `format_size(bytes)` with `format_size_for_system(bytes, &system)` at:

1. **`rom_list.rs:476`** -- the system is available as `rom.game.system`
2. **`game_detail.rs:55`** -- the system is available as `game.system` (via `detail.game.system`)

**Do NOT change:**

3. **`system_card.rs:12`** -- total size per system should stay in MB/GB (it is aggregate disk usage)
4. **`home.rs:96-97`** -- storage bar is about physical disk usage, always MB/GB
5. **`more.rs:38-40`** -- disk stats, always MB/GB
6. **`metadata.rs`** -- DB and media sizes, always MB/GB

---

## 5. Edge Cases

### Large cartridge ROMs

N64 ROMs can reach 64 MB (512 Mbit). Neo Geo ROMs can reach 86 MB (688 Mbit). These numbers are historically accurate -- Neo Geo games really did advertise "100 MEGA SHOCK!" (100 Mbit) and larger. "512 Mbit" is a reasonable display for Conker's Bad Fur Day.

**Verdict:** No special case needed. Large Mbit values are authentic.

### Very small ROMs

Atari 2600 ROMs are typically 2 KB -- 32 KB (16 Kbit -- 256 Kbit). Displaying "32 Kbit" is appropriate and matches how these systems are discussed by enthusiasts.

At the extreme low end (under 1 Kbit), fall back to showing bytes. This is unlikely in practice.

**Verdict:** Use Kbit for values under 1 Mbit, bytes for values under 128 bytes (1 Kbit).

### Nintendo DS

The DS used cartridge-format game cards, but by 2004 the industry had shifted to discussing sizes in Megabytes. DS game cards were sold as "64 MB", "128 MB", "256 MB" -- not in Megabit. Additionally, DS ROM dumps are large (32 MB -- 512 MB), and showing "4096 Mbit" for a 512 MB game is unfamiliar.

**Verdict:** Keep DS in MB/GB. The `uses_megabit()` method should return `false` for `nintendo_ds`.

### MSX cartridge vs. floppy

MSX has both cartridge ROMs (`.rom`) and floppy images (`.dsk`). The system definition includes both extensions. Since floppy images should not be shown in Mbit, there are two options:
- Classify MSX as non-Mbit (simpler, floppy images are common)
- Check the file extension and use Mbit only for `.rom` files (more accurate but more complex)

**Verdict:** Classify MSX as non-Mbit. Simplicity wins; most MSX content in collections is floppy-based.

### M3U files (multi-disc playlists)

M3U files point to multiple disc images. Their own file size is negligible (a few hundred bytes). The app currently sums the sizes of referenced files into `RomEntry.size_bytes`. Since M3U is only used for disc-based systems (PSX, Sega CD, Saturn, etc.), these will always display in MB/GB.

**Verdict:** No special handling needed.

### Compressed ROMs (.zip, .7z)

The displayed `size_bytes` is the on-disk file size (compressed). For disc-based systems this is always MB/GB. For cartridge systems, the compressed size is smaller than the original ROM. A 16 Mbit Mega Drive ROM might compress to 500 KB, which would display as "4 Mbit" instead of "16 Mbit."

This is a display inaccuracy, but one that applies to the current MB display too -- a compressed SNES ROM shows as "500 KB" rather than "2 MB." The vast majority of cartridge ROMs in collections are stored uncompressed (bare `.smd`, `.smc`, `.z64` files), while arcade ROMs are always zipped.

**Verdict:** Accept the compressed-size display for now. A future enhancement could detect zipped ROMs and show the uncompressed ROM size, but this requires reading zip metadata and is out of scope.

### Arcade ZIP files

Arcade ROMs (`arcade_fbneo`, `arcade_mame`, `arcade_mame_2k3p`) are always distributed as ZIP files containing multiple ROM chip dumps. The total size of the chips is what matters for Mbit display, but the ZIP file is compressed.

The compressed ZIP size displayed in Mbit would be inaccurate (e.g., a board with 40 Mbit of ROMs might compress to 3 MB = 24 Mbit displayed). However, arcade ZIP compression ratios are low for ROM data, so the discrepancy is moderate.

**Options:**
1. Show compressed ZIP size in Mbit anyway (inaccurate but simple)
2. Show arcade ZIPs in MB instead (accurate for disk usage, loses historical context)
3. Read the ZIP central directory to get uncompressed total (accurate but I/O-heavy for large lists)

**Verdict:** Show arcade ROMs in Mbit based on compressed file size. The inaccuracy is acceptable -- most users will see plausible numbers (within 10-30% of the real board size), and the Mbit unit itself conveys the right cultural context. If accuracy becomes important, reading uncompressed sizes from ZIP metadata can be added later.

### Atomiswave/Naomi (arcade_dc)

These arcade systems used GD-ROM or flash storage with files typically hundreds of MB. They are more akin to disc-based systems in terms of user expectation. The ZIP/CHD files are large.

**Verdict:** Keep `arcade_dc` in MB/GB. The `uses_megabit()` method returns `false` for `arcade_dc`.

---

## 6. Display Format

### Option A: Mbit only (recommended)

Show the historically relevant unit alone:

```
ROM list:   "16 Mbit"     (cartridge)    "450 MB"     (disc)
Detail:     "16 Mbit"     (cartridge)    "450.2 MB"   (disc)
```

**Pros:** Clean, concise, authentic to the era. Retro gaming enthusiasts instantly recognize "16 Mbit" as a Mega Drive game size.

**Cons:** Casual users unfamiliar with Megabit may be confused. Not directly comparable with disk usage stats shown in MB/GB on the home page.

### Option B: Mbit with MB in parentheses

Show both units:

```
ROM list:   "16 Mbit (2 MB)"
Detail:     "16 Mbit (2.0 MB)"
```

**Pros:** Educational, no confusion about actual file size.

**Cons:** Verbose, clutters the compact ROM list row. The parenthetical MB is redundant for anyone who knows what Mbit means.

### Option C: Context-dependent dual display

Show Mbit only in ROM list (space-constrained), show both on game detail page (more room):

```
ROM list:   "16 Mbit"
Detail:     "16 Mbit (2.0 MB)"
```

**Pros:** Best of both worlds. The detail page has room for the extra info.

**Cons:** Inconsistency between list and detail views.

### Recommendation: Option A

Use Option A. The companion app is purpose-built for retro gaming enthusiasts. All five user personas in the user analysis are people who chose to set up a Raspberry Pi with RetroArch -- even the "casual" persona grew up with these consoles. "16 Mbit" is not obscure jargon to this audience; it is authentic period detail that adds character.

If a user does not know what Mbit means, the context (a game's file size) makes it self-explanatory enough. And the system card / storage bar still shows MB/GB for disk usage, keeping practical information in practical units.

---

## 7. Where Each Unit Appears (Summary)

| UI Location | Current | Proposed | Rationale |
|---|---|---|---|
| ROM list (per-game size) | MB/GB | Mbit or MB/GB per system | Per-game size in historical unit |
| Game detail (file size field) | MB/GB | Mbit or MB/GB per system | Same as ROM list |
| System card (total size) | MB/GB | MB/GB (no change) | Aggregate disk usage |
| Home page storage bar | MB/GB | MB/GB (no change) | Physical disk usage |
| More page disk stats | MB/GB | MB/GB (no change) | Physical disk usage |
| Metadata page DB/media size | MB/GB | MB/GB (no change) | Infrastructure sizes |
| Search results | (not shown) | (not shown) | N/A |

---

## 8. System-to-Unit Mapping (Complete)

For the `uses_megabit()` implementation:

```rust
/// Systems whose ROM sizes should be displayed in Megabit (Mbit/Kbit).
const MEGABIT_SYSTEMS: &[&str] = &[
    // Cartridge consoles
    "atari_2600",
    "atari_5200",
    "atari_7800",
    "atari_jaguar",
    "nintendo_nes",
    "nintendo_snes",
    "nintendo_n64",
    "nec_pce",
    "sega_sg",
    "sega_sms",
    "sega_smd",
    "sega_32x",
    "snk_ng",
    // Cartridge handhelds
    "atari_lynx",
    "nintendo_gb",
    "nintendo_gbc",
    "nintendo_gba",
    "sega_gg",
    "snk_ngp",
    // Arcade (ROM-chip based)
    "arcade_fbneo",
    "arcade_mame",
    "arcade_mame_2k3p",
];
```

Systems explicitly NOT in the list (MB/GB):
- `nintendo_ds` -- DS era used MB, not Mbit
- `arcade_dc` -- disc/flash-based arcade
- `sony_psx`, `sega_dc`, `sega_cd`, `sega_st` -- optical disc
- `nec_pcecd`, `panasonic_3do`, `philips_cdi`, `snk_ngcd` -- optical disc
- `commodore_ami`, `commodore_amicd`, `commodore_c64` -- floppy/tape/disc
- `ibm_pc`, `amstrad_cpc`, `sinclair_zx`, `sharp_x68k` -- floppy/tape
- `microsoft_msx` -- mixed cart/floppy, defaulting to MB
- `scummvm` -- PC game data
- `alpha_player` -- media files

---

## 9. User Persona Impact

From the user analysis (`docs/reference/user-analysis.md`):

**Persona A (Casual Retro Gamer):** Grew up with 8/16-bit consoles. Would recognize "16 Mbit" from the original cartridge labels. Adds nostalgic authenticity.

**Persona B (Collector/Curator):** Cares about organization and accuracy. Would appreciate historically correct units as a mark of quality and attention to detail.

**Persona C (Parent/Family):** Unlikely to notice or care about size units. Does not create friction -- "16 Mbit" is no harder to understand than "2.0 MB" when the user does not care about either.

**Persona D (Arcade Cabinet Builder):** Would appreciate Mbit for arcade ROM sizes. Arcade enthusiasts know their boards in Megabits -- "CPS2: 160 Mbit" is a meaningful reference. This persona benefits most directly.

**Persona E (Technical User):** Might prefer MB for consistency with disk metrics. However, the system card and storage views still use MB/GB, so practical disk information remains in practical units.

**Verdict:** No persona is harmed by this change. Personas A, B, and D benefit. Personas C and E are neutral.

---

## 10. Implementation Checklist

1. Add `uses_megabit()` method to `System` in `replay-control-core/src/systems.rs`
2. Add `find_system_uses_megabit(folder_name: &str) -> bool` public function for use from the app crate
3. Add `format_size_megabit(bytes) -> String` to `replay-control-app/src/util.rs`
4. Add `format_size_for_system(bytes, system) -> String` to `replay-control-app/src/util.rs`
5. Update `rom_list.rs:476` to use `format_size_for_system(rom.size_bytes, &rom.game.system)`
6. Update `game_detail.rs:55` to use `format_size_for_system(detail.size_bytes, &detail.game.system)`
7. Add unit tests for Mbit formatting (edge cases: Kbit, exact Mbit, fractional Mbit, large values)
8. Do NOT change: system_card.rs, home.rs, more.rs, metadata.rs

### Test cases

```
format_size_megabit(2048)          -> "16 Kbit"       (2 KB Atari 2600 ROM)
format_size_megabit(4096)          -> "32 Kbit"       (4 KB Atari 2600 ROM)
format_size_megabit(131_072)       -> "1 Mbit"        (128 KB)
format_size_megabit(262_144)       -> "2 Mbit"        (256 KB)
format_size_megabit(524_288)       -> "4 Mbit"        (512 KB = 4 Mbit, classic SMS/GG)
format_size_megabit(1_048_576)     -> "8 Mbit"        (1 MB = 8 Mbit, SMW on SNES)
format_size_megabit(2_097_152)     -> "16 Mbit"       (2 MB = 16 Mbit, Sonic 3)
format_size_megabit(3_145_728)     -> "24 Mbit"       (3 MB = 24 Mbit, Phantasy Star IV)
format_size_megabit(4_194_304)     -> "32 Mbit"       (4 MB = 32 Mbit, DKC on SNES)
format_size_megabit(8_388_608)     -> "64 Mbit"       (8 MB = 64 Mbit, Super Mario 64)
format_size_megabit(33_554_432)    -> "256 Mbit"      (32 MB = 256 Mbit, RE2 on N64)
format_size_megabit(67_108_864)    -> "512 Mbit"      (64 MB = 512 Mbit, Conker)
format_size_megabit(786_432)       -> "6 Mbit"        (768 KB -- rounds to whole)
format_size_megabit(655_360)       -> "5.0 Mbit"      (640 KB -- shows decimal if not whole)
```

---

## 11. Internationalization

The current i18n key for the file size label is `game_detail.file_size`. This label ("File Size" / "Size") does not need to change -- "16 Mbit" is the value, not the label.

The unit abbreviations "Mbit", "Kbit", "MB", "GB", "KB" are internationally recognized and do not need translation.

No i18n changes required.

---

## 12. Future Enhancements (Out of Scope)

- **Show uncompressed ROM size for ZIP files**: Read ZIP central directory to get the total uncompressed size. Would make arcade ROM Mbit values accurate. Adds I/O cost.
- **ROM chip breakdown for arcade**: Show individual ROM chip names and sizes from the MAME XML. Deep arcade enthusiast feature.
- **Dual-unit toggle**: Let users choose between Mbit and MB in settings. Adds complexity for marginal benefit.
- **Sort by size in Mbit**: If sort-by-size is added to ROM lists, sort by bytes internally regardless of display unit.
