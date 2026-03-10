# Cross-Repo Arcade Image Search & Dreamcast GDI Naming Analysis

Date: 2026-03-10
Status: Implemented (Naomi multi-repo + DC version stripping)

## Part 1: Arcade Cross-Repo Thumbnail Search

### Current Behavior

`thumbnail_repo_name()` returns a single repo per system:

| System            | Maps to                    |
|-------------------|----------------------------|
| `arcade_mame`     | `MAME`                     |
| `arcade_fbneo`    | `FBNeo - Arcade Games`     |
| `arcade_mame_2k3p`| `MAME`                     |
| `arcade_dc`       | `Atomiswave`               |

### The Problem

ROM sets across arcade systems overlap significantly, but thumbnail repos don't.

**MAME vs FBNeo repos:**
- MAME repo: 5,866 boxart entries
- FBNeo repo: 6,473 boxart entries
- Shared (exact filename): 2,942 entries
- MAME-only: 2,924 entries
- FBNeo-only: 3,531 entries

Since display names are used (via `arcade_db`), the same ROM (`akatana.zip`) translates to the same display name regardless of which system it's in. The real question is: does the matching thumbnail exist in the *other* repo?

**Confirmed example:** `akatana` (Akai Katana) exists in the FBNeo ROM set. Its display name `"Akai Katana (2010_ 8_13 MASTER VER.)"` exists in the MAME boxart repo but NOT in the FBNeo repo. So `arcade_fbneo` games currently miss this thumbnail.

**ROM set overlap is actually small on disk.** On the NFS mount:
- `arcade_mame`: 763 ROMs (555 horizontal + 208 vertical, clean sets only)
- `arcade_fbneo`: 2,139 ROMs (1,380 horizontal + 759 vertical)
- Overlapping ROMs: only 11 (9 horizontal + 2 vertical)

So the overlap is tiny in the actual ROM collection, but the *thumbnail* coverage gap is much larger because each repo has ~3,000 unique entries the other doesn't.

**arcade_dc (Atomiswave + Naomi):**
- Currently maps to `Atomiswave` only
- Actual ROMs: 24 Atomiswave + 148 Naomi = 172 total
- Atomiswave repo: 33 boxart entries (good coverage for 24 ROMs)
- Naomi repo: 116 boxart entries (good coverage for 148 ROMs)
- Naomi 2 repo: 11 boxart entries (minimal, but a few games overlap)
- **All Naomi ROMs currently get zero images** because the code only checks Atomiswave

### Cost Analysis

Cloning additional repos per system:

| Repo                    | GitHub size  | PNG files | Clone time (est.) |
|-------------------------|-------------|-----------|-------------------|
| MAME                    | ~11.7 GB    | 35,054    | Very slow          |
| FBNeo - Arcade Games    | ~7.1 GB     | 28,856    | Slow               |
| Atomiswave              | ~44 MB      | 101       | Fast               |
| Sega - Naomi            | ~163 MB     | 376       | Fast               |
| Sega - Naomi 2          | ~14 MB      | 70        | Fast               |

**MAME + FBNeo cross-search:** Expensive. Both repos are massive (11.7 GB + 7.1 GB). Cloning both for every import doubles bandwidth and disk. On a Pi with limited `/tmp`, this is problematic.

**Atomiswave + Naomi cross-search:** Cheap. Adding Naomi (163 MB) to the existing Atomiswave (44 MB) clone is trivial. Adding Naomi 2 (14 MB) is negligible.

### Recommendation

**For `arcade_dc`: Definitely add Naomi and Naomi 2 repos.** This is a clear win:
- 148 Naomi ROMs currently get zero thumbnails
- Naomi repo is only 163 MB
- The code already handles the display name translation via `arcade_db`
- Recommended priority: Atomiswave first (for Atomiswave ROMs), then Naomi, then Naomi 2

**For MAME/FBNeo cross-search: Not worth it now.** The cost is high (cloning ~19 GB total) and the benefit is marginal since the actual ROM overlap is only 11 games. However, if a user has a game like `akatana` in their FBNeo set, they miss its thumbnail. Two possible approaches:

1. **Fallback approach (recommended if pursued):** Try the primary repo first, then clone the secondary only if there are missing images. This avoids the upfront cost but adds complexity.
2. **Pre-built combined index:** Ship a static mapping of "display name -> which repos have it" at build time, so you know *before* cloning whether the fallback repo would help.

For now, the 11-game overlap doesn't justify the implementation complexity or the bandwidth cost.

### Code Changes Required

**For `arcade_dc` (minimal change):**

Replace `thumbnail_repo_name()` returning `Option<&str>` with a new function `thumbnail_repo_names()` returning `Vec<&str>`:

```
"arcade_dc" => vec!["Atomiswave", "Sega - Naomi", "Sega - Naomi 2"],
```

Or keep the existing function for most systems and add a `thumbnail_fallback_repos()`:

```
"arcade_dc" => Some(&["Sega - Naomi", "Sega - Naomi 2"]),
```

The caller in `api/mod.rs` would need to loop over repos, cloning and importing from each. The `import_system_thumbnails` function already skips ROMs that already have images (the `!dst.exists()` check), so running it against multiple repos in sequence works naturally.

**For MAME/FBNeo (if pursued later):**

```
"arcade_fbneo" => vec!["FBNeo - Arcade Games", "MAME"],
"arcade_mame" => vec!["MAME", "FBNeo - Arcade Games"],
```

Primary repo listed first for priority ordering.

---

## Part 2: Dreamcast GDI Naming Issue

### Current Behavior

DC ROMs on the NFS mount use the TOSEC/Trurip GDI naming convention:

```
Sonic Adventure 2 v1.008 (2001)(Sega)(PAL)(M5)[!]
```

Each ROM is a directory containing:
- `<name>.gdi` - the GDI descriptor file
- `track01.bin`, `track02.raw`, etc. - the actual data tracks

The `list_rom_filenames` function descends recursively and collects individual files, so it finds the `.gdi` file inside the directory. The stem becomes:

```
Sonic Adventure 2 v1.008 (2001)(Sega)(PAL)(M5)[!]
```

The existing `strip_tags` function finds the first ` (` and strips everything after:

```
"Sonic Adventure 2 v1.008 (2001)(Sega)(PAL)(M5)[!]"  ->  "Sonic Adventure 2 v1.008"
```

### The Problem

The libretro-thumbnails "Sega - Dreamcast" repo uses No-Intro/Redump naming:

```
Sonic Adventure 2 (Europe) (En,Ja,Fr,De,Es).png
```

After `strip_tags` on the repo entry: `"Sonic Adventure 2"`.
After `strip_tags` on the ROM: `"Sonic Adventure 2 v1.008"`.

The version string `v1.008` prevents matching.

### Affected ROMs

**22 of 23 DC ROMs** use the `vX.XXX (Year)(Publisher)(Region)` convention. Only one uses Redump-style naming:

```
Super Street Fighter II X for Matching Service (Japan)   <-- matches directly
```

The GDI-format ROMs have these version patterns:
- `v1.000`, `v1.001`, `v1.002`, `v1.003`, `v1.004`, `v1.005`, `v1.008`, `v1.009`, `v1.021`
- `v2.000` (Space Channel 5 Part 2)
- `v1 001` (Sega Rally 2 -- note: space instead of dot in the directory name, but the .gdi file inside uses `v1.001`)

### Additional Naming Issues

Beyond the version string, there are other mismatches:

1. **MSR / Metropolis Street Racer**: ROM is `"Metropolis Street Racer"` but repo uses `"MSR - Metropolis Street Racer"`. Version stripping alone won't fix this.

2. **Super Street Fighter IIX vs II X**: The GDI ROM uses `"Super Street Fighter IIX for Matching Service - Grand Master Challenge"` but the repo has `"Super Street Fighter II X for Matching Service"`. Different title variant AND extra subtitle.

### Proposed Fix: Strip Version Strings

Add a `strip_gdi_version` step in the fuzzy matching pipeline for Dreamcast ROMs. The version pattern is consistent:

```
 v\d[\d. ]*$   (at end of the stripped name)
```

Examples:
| ROM stem after strip_tags          | After strip version     | Repo match?                                      |
|------------------------------------|-------------------------|--------------------------------------------------|
| `Sonic Adventure 2 v1.008`         | `Sonic Adventure 2`     | Yes: `Sonic Adventure 2 (Europe) ...`             |
| `Crazy Taxi v1.000`                | `Crazy Taxi`            | Yes: `Crazy Taxi (USA).png`                       |
| `Jet Set Radio v1.002`             | `Jet Set Radio`         | Yes: `Jet Set Radio (Europe) ...`                 |
| `Power Stone 2 v1.000`             | `Power Stone 2`         | Yes: `Power Stone 2 (Europe).png`                 |
| `Virtua Tennis 2 v1.009`           | `Virtua Tennis 2`       | Partial: repo has `Virtua Tennis 2 - Sega Professional Tennis (Europe) ...` (needs strip_tags on repo side too, which already happens) |
| `Daytona USA 2001 v1.002`          | `Daytona USA 2001`      | Yes: `Daytona USA 2001 (Europe) ...`              |
| `Metropolis Street Racer v1.009`   | `Metropolis Street Racer` | No: repo uses `MSR - Metropolis Street Racer` |

**Expected improvement:** 19 of 22 GDI ROMs would match after version stripping. The remaining 3 failures are:
- `Metropolis Street Racer` -- different title prefix in repo (`MSR - ...`)
- `Super Street Fighter IIX` -- different title variant (`II X` vs `IIX`) and extra subtitle
- `Puyo Puyo Fever` -- repo has `Puyo Pop Fever (World)` not `Puyo Puyo Fever`

### Implementation Options

**Option A: Add version stripping to `strip_tags` (broadly)**

Modify `strip_tags` to also strip trailing ` vX.XXX` patterns. This would affect all systems, but the pattern ` v\d` is specific enough to GDI dumps that it's unlikely to cause false positives (no other system uses this convention in filenames).

**Option B: System-specific stripping in `import_system_thumbnails`**

Add a `strip_gdi_version` function and apply it only when `system == "sega_dc"`. This is safer but adds a special case.

**Option C: Add a third tier to fuzzy matching**

Currently: exact match -> strip_tags match. Add: -> strip_tags + strip_version match. This keeps backward compatibility and adds coverage.

**Recommended: Option C** -- add a third matching tier. The fuzzy index already uses `strip_tags` on both the ROM name and repo entries. Adding version stripping as an additional fallback is clean and doesn't risk breaking existing matches.

The regex for version stripping: ` v\d[\d._ ]*$` -- matches ` v` followed by a digit and any combination of digits, dots, underscores, and spaces until end-of-string. This handles both `v1.008` and `v1 001` (the Sega Rally edge case).

### Side Note: DC Repo Also Has TOSEC-Named Entries

A few entries in the Dreamcast thumbnail repo use the same GDI/TOSEC naming:

```
Jojo's Bizarre Adventure v1.001 (2000)(Virgin)(PAL)[!].png
Puzzle Bobble 4 v1.000 (2000)(Cyberfront - Taito)(JP)[!].png
Samba De Amigo v1.002 (2000)(Sega)(PAL)[!].png
```

The existing exact matching would already find these. The version-stripping fallback should be careful not to demote an exact TOSEC match to a fuzzy Redump match. Since the current code tries exact match first (line 143-146 in thumbnails.rs), this is already handled correctly.
