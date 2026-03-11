# Metadata Population Analysis

Analysis of metadata coverage in the `metadata.db` SQLite database.

**Date:** March 2026
**Database:** `.replay-control/metadata.db` on NFS mount (16 MB, 18,919 entries)

---

## 1. Schema

Single table `game_metadata` with columns:

| Column | Type | Nullable | Notes |
|---|---|---|---|
| `system` | TEXT | NOT NULL | Primary key part 1 |
| `rom_filename` | TEXT | NOT NULL | Primary key part 2 |
| `description` | TEXT | nullable | Game overview/synopsis |
| `rating` | REAL | nullable | Community rating (0-5 scale) |
| `publisher` | TEXT | nullable | Publisher name |
| `source` | TEXT | NOT NULL | `"launchbox"` or `"thumbnails"` |
| `fetched_at` | INTEGER | NOT NULL | Unix timestamp |
| `box_art_path` | TEXT | nullable | Relative path to box art image |
| `screenshot_path` | TEXT | nullable | Relative path to screenshot |

---

## 2. Overall Field Coverage

Total entries: **18,919**

| Field | Populated | Empty/NULL | Coverage |
|---|---:|---:|---:|
| `system` | 18,919 | 0 | **100.0%** |
| `rom_filename` | 18,919 | 0 | **100.0%** |
| `source` | 18,919 | 0 | **100.0%** |
| `fetched_at` | 18,919 | 0 | **100.0%** |
| `rating` | 15,630 | 3,289 | **82.6%** |
| `description` | 14,820 | 4,099 | **78.3%** |
| `screenshot_path` | 14,592 | 4,327 | **77.1%** |
| `publisher` | 14,555 | 4,364 | **76.9%** |
| `box_art_path` | 12,256 | 6,663 | **64.8%** |

**Least populated field: `box_art_path` at 64.8%.** The four optional metadata fields (`publisher`, `description`, `screenshot_path`, `rating`) cluster around 77-83%. `box_art_path` is a clear outlier at 65%.

---

## 3. Coverage by Source

Two data sources populate the database:

| Source | Entries | description | rating | publisher | box_art | screenshot |
|---|---:|---:|---:|---:|---:|---:|
| `launchbox` | 16,354 | 14,820 (90.6%) | 15,630 (95.6%) | 14,555 (89.0%) | 10,022 (61.3%) | 12,053 (73.7%) |
| `thumbnails` | 2,565 | 0 (0%) | 0 (0%) | 0 (0%) | 2,234 (87.1%) | 2,539 (99.0%) |

Key observations:
- **LaunchBox** provides text metadata (description, rating, publisher) but has significant gaps in image paths (only 61% box art, 74% screenshots).
- **Thumbnails** source provides ONLY image paths -- no text metadata at all. This is expected since the thumbnail matcher just links existing image files.
- 2,565 entries exist only because they matched a thumbnail but had no LaunchBox match (otherwise they'd have source `launchbox` with thumbnail paths populated).

---

## 4. Per-System Coverage

Sorted by average coverage (across the 5 optional fields):

| System | Entries | Avg Coverage | description | rating | publisher | box_art | screenshot |
|---|---:|---:|---:|---:|---:|---:|---:|
| sega_dc | 22 | **39.1%** | 4.5% | 4.5% | 4.5% | 86.4% | 95.5% |
| sharp_x68k | 1,632 | **49.3%** | 83.9% | 65.5% | 97.2% | 0.0% | 0.0% |
| sega_sms | 759 | **59.6%** | 99.7% | 99.7% | 98.4% | 0.0% | 0.0% |
| sega_sg | 169 | **59.9%** | 100.0% | 100.0% | 99.4% | 0.0% | 0.0% |
| sega_gg | 514 | **60.0%** | 100.0% | 100.0% | 100.0% | 0.0% | 0.0% |
| sega_st | 40 | **60.0%** | 100.0% | 100.0% | 100.0% | 0.0% | 0.0% |
| arcade_mame | 3,908 | **62.4%** | 52.1% | 77.7% | 48.4% | 48.0% | 86.1% |
| sega_smd | 2,764 | **82.0%** | 82.7% | 83.1% | 78.8% | 82.7% | 82.8% |
| arcade_dc | 143 | **84.1%** | 79.7% | 80.4% | 79.7% | 81.8% | 98.6% |
| nintendo_snes | 4,200 | **87.2%** | 81.9% | 81.8% | 80.6% | 95.5% | 96.0% |
| arcade_fbneo | 4,061 | **87.5%** | 86.6% | 89.1% | 82.6% | 79.8% | 99.7% |
| nintendo_n64 | 624 | **87.9%** | 80.8% | 80.4% | 80.1% | 99.8% | 98.6% |
| sega_cd | 25 | **92.0%** | 88.0% | 88.0% | 84.0% | 100.0% | 100.0% |
| sega_32x | 50 | **94.0%** | 90.0% | 90.0% | 90.0% | 100.0% | 100.0% |
| ibm_pc | 8 | **97.5%** | 100.0% | 100.0% | 100.0% | 100.0% | 87.5% |

---

## 5. Problem Areas

### 5.1 Systems with 0% image coverage (no thumbnails matched)

Five systems have LaunchBox text metadata but **zero** box art and screenshot paths:

| System | Entries | Has description | Has box_art | Has screenshot |
|---|---:|---:|---:|---:|
| sharp_x68k | 1,632 | 83.9% | 0.0% | 0.0% |
| sega_sms | 759 | 99.7% | 0.0% | 0.0% |
| sega_gg | 514 | 100.0% | 0.0% | 0.0% |
| sega_sg | 169 | 100.0% | 0.0% | 0.0% |
| sega_st | 40 | 100.0% | 0.0% | 0.0% |

**Root cause:** These systems have no thumbnail repositories downloaded, so the thumbnail matcher never runs for them. The LaunchBox import provides text metadata only.

### 5.2 Sega Dreamcast: almost no text metadata

`sega_dc` has 22 entries but only 1 came from LaunchBox (the rest are thumbnail-only). This means 95.5% of Dreamcast entries have screenshots but no description, rating, or publisher.

### 5.3 MAME arcade: lowest text coverage among large systems

`arcade_mame` (3,908 entries) has the lowest text coverage of the large systems:
- description: 52.1% (vs 86.6% for FBNeo)
- publisher: 48.4% (vs 82.6% for FBNeo)
- box_art: 48.0% (vs 79.8% for FBNeo)

785 of 3,908 MAME entries come from thumbnails only (no LaunchBox match). Even among the 3,123 LaunchBox entries, coverage is lower than FBNeo, likely because MAME has many obscure/regional entries that LaunchBox doesn't cover well.

### 5.4 Systems on disk with zero metadata

Two system directories on the NFS storage have ROM files but no metadata entries at all:

| System | ROMs on disk | Metadata entries |
|---|---:|---:|
| scummvm | 146 | 0 |
| commodore_ami | 1 | 0 |

Additionally, many systems mapped in `launchbox.rs` have no ROMs present on this storage, so they have no metadata entries despite being supported (e.g., `nintendo_nes`, `nintendo_gb`, `sony_psx`, `atari_*`, `snk_*`, etc.).

---

## 6. Rating Distribution

Among entries with a rating (15,630 total):

| Range | Count | % of rated |
|---|---:|---:|
| 0.01 - 1.0 | 219 | 1.4% |
| 1.01 - 2.0 | 738 | 4.7% |
| 2.01 - 3.0 | 4,122 | 26.4% |
| 3.01 - 4.0 | 8,070 | 51.6% |
| 4.01 - 5.0 | 2,481 | 15.9% |

Ratings skew positive (68% rated 3.0+), which is typical for community-sourced game ratings.

---

## 7. Summary and Recommendations

**Biggest gaps by field:**
1. **box_art_path** (64.8%) -- lowest coverage of any field. Driven by five systems with 0% image coverage and MAME at 48%.
2. **publisher** (76.9%) -- missing for all thumbnail-only entries and ~10% of LaunchBox entries.
3. **screenshot_path** (77.1%) -- same five systems at 0% plus LaunchBox gaps.
4. **description** (78.3%) -- similar pattern to publisher.

**Biggest gaps by system:**
1. **sega_dc** (39.1% avg) -- almost entirely thumbnail-only, needs LaunchBox matching improvement.
2. **sharp_x68k** (49.3% avg) -- good text metadata, zero images. Needs thumbnail repos.
3. **sega_sms/gg/sg/st** (59-60% avg) -- excellent text metadata, zero images. Needs thumbnail repos.
4. **arcade_mame** (62.4% avg) -- weakest large system across all fields.

**Actionable improvements:**
- Download thumbnail repositories for sega_sms, sega_gg, sega_sg, sega_st, and sharp_x68k to fill the image gap (would add ~3,114 potential image matches).
- Investigate why sega_dc has only 1 LaunchBox match out of 22 entries.
- Review MAME LaunchBox matching -- the 52% description rate suggests many entries aren't matching despite having LaunchBox data available.
