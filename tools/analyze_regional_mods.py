#!/usr/bin/env python3
"""Analyze regional modifications in the NFS ROM collection.

Scans ROM filenames, classifies them using the same logic as rom_tags.rs,
groups by base_title + system, and identifies cases where multiple ROMs
share the same base_title AND region — "regional modifications".

These are ROMs like:
  - "Game (USA)" + "Game (USA) (Traducido Es)" — same region, translation patch
  - "Game (Europe)" + "Game (Europe) (FastRom)" — same region, performance patch
  - "Game (USA)" + "Game (USA) (Rev 1)" — same region, revision
"""

import os
import re
import sys
from collections import defaultdict
from pathlib import Path

# =============================================================================
# Configuration
# =============================================================================

NFS_ROMS = Path("<NFS_MOUNT>/roms")

# Directories at the system level that are NOT systems (start with _)
SKIP_PREFIXES = ("_",)

# =============================================================================
# Tag parsing (mirrors rom_tags.rs logic)
# =============================================================================

# Known region names
REGIONS = {
    "usa", "europe", "japan", "world", "spain", "france", "germany",
    "italy", "brazil", "korea", "taiwan", "china", "australia", "asia",
    "russia", "argentina", "netherlands", "sweden", "scandinavia", "uk", "canada",
}

# GoodTools single-letter codes
GOODTOOLS_CHARS = set("JUEBKW")

# Language-only codes (noise, not regions)
LANG_CODES = {
    "en", "fr", "de", "es", "it", "ja", "pt", "nl", "sv", "no", "da",
    "fi", "ko", "zh", "ru", "pl", "hu", "cs", "ca", "ro", "tr", "ar", "pt-br",
}

# Noise tags
NOISE_TAGS = {
    "virtual console", "switch online", "virtual console, switch online",
    "virtual console, classic mini, switch online", "virtual console, classic mini",
    "classic mini", "np", "bs", "program", "sample", "ntsc", "pal",
    "sufami turbo", "seganet", "sega channel", "fixed", "alt", "final",
    "update", "steam", "collection of mana", "mega man x legacy collection",
    "game no kanzume otokuyou", "pd", "vc", "j-cart", "nintendo super system",
    "mame snes bootleg", "unknown",
}


def extract_paren_tags(stem):
    """Extract all (...) tags from a filename stem."""
    tags = []
    i = 0
    while i < len(stem):
        open_idx = stem.find('(', i)
        if open_idx == -1:
            break
        close_idx = stem.find(')', open_idx + 1)
        if close_idx == -1:
            break
        tags.append(stem[open_idx + 1:close_idx])
        i = close_idx + 1
    return tags


def extract_bracket_tags(stem):
    """Extract all [...] tags from a filename stem."""
    tags = []
    i = 0
    while i < len(stem):
        open_idx = stem.find('[', i)
        if open_idx == -1:
            break
        close_idx = stem.find(']', open_idx + 1)
        if close_idx == -1:
            break
        tags.append(stem[open_idx + 1:close_idx])
        i = close_idx + 1
    return tags


def is_language_code(s):
    return s.lower() in LANG_CODES


def is_noise_tag(lower):
    parts = [p.strip() for p in lower.split(',')]
    if all(is_language_code(p) for p in parts):
        return True
    return lower in NOISE_TAGS


def looks_like_region(tag):
    lower = tag.lower()

    # GoodTools compact codes: U, E, J, W, UE, JU, etc.
    if len(tag) <= 5 and all(c in GOODTOOLS_CHARS for c in tag):
        return True

    # Multi-region like "USA, Europe"
    parts = [p.strip() for p in lower.split(',')]
    if all(p in REGIONS or is_language_code(p) for p in parts):
        if all(is_language_code(p) for p in parts):
            return False
        return True

    return False


def region_to_priority(tag):
    """Map a region tag to a priority string (matches rom_tags.rs)."""
    lower = tag.lower()
    parts = [p.strip() for p in lower.split(',')]
    first = parts[0] if parts else ""

    if first in ("world", "w"):
        return "world"
    elif first in ("usa", "u") or first == "usa, europe" or first == "ue":
        return "usa"
    elif first in ("europe", "e"):
        return "europe"
    elif first in ("japan", "j"):
        return "japan"
    elif "usa" in first:
        return "usa"
    elif "europe" in first:
        return "europe"
    elif "japan" in first:
        return "japan"
    else:
        return "other"


def expand_region_code(code):
    """Expand compact region codes like 'UE' to 'USA, Europe'."""
    parts = []
    for c in code:
        if c == 'J':
            parts.append("Japan")
        elif c == 'U':
            parts.append("USA")
        elif c == 'E':
            parts.append("Europe")
        elif c == 'B':
            parts.append("Brazil")
        elif c == 'K':
            parts.append("Korea")
        elif c == 'W':
            parts.append("World")
        else:
            return ""
    return ", ".join(parts)


def normalize_region(region):
    if len(region) <= 5 and all(c.isupper() for c in region):
        expanded = expand_region_code(region)
        if expanded:
            return expanded
    return region


def parse_revision(tag):
    lower = tag.lower()
    if lower.startswith("rev "):
        rest = tag[4:].strip()
        if rest:
            return f"Rev {rest}"
    if lower.startswith("rev") and len(lower) >= 5:
        rest = tag[3:]
        if rest.isdigit():
            return f"Rev {int(rest)}"
    return None


TRANSLATION_LANG_MAP = {
    "en": "EN", "eng": "EN", "english": "EN",
    "es": "ES", "spa": "ES", "spanish": "ES", "espanol": "ES",
    "fr": "FR", "fre": "FR", "french": "FR", "fra": "FR",
    "de": "DE", "ger": "DE", "german": "DE", "deu": "DE",
    "it": "IT", "ita": "IT", "italian": "IT",
    "pt": "PT", "por": "PT", "portuguese": "PT",
    "bra": "PT-BR", "pt-br": "PT-BR",
    "ru": "RU", "rus": "RU", "russian": "RU",
    "ja": "JA", "jpn": "JA", "japanese": "JA",
    "ko": "KO", "kor": "KO", "korean": "KO",
    "zh": "ZH", "chi": "ZH", "chinese": "ZH",
    "sv": "SV", "swe": "SV", "swedish": "SV",
    "pl": "PL", "pol": "PL", "polish": "PL",
    "nl": "NL", "dut": "NL", "dutch": "NL",
    "el": "EL", "gre": "EL", "greek": "EL",
    "no": "NO", "nor": "NO", "norwegian": "NO",
    "da": "DA", "dan": "DA", "danish": "DA",
    "fi": "FI", "fin": "FI", "finnish": "FI",
    "hu": "HU", "hun": "HU", "hungarian": "HU",
    "cs": "CS", "cze": "CS", "czech": "CS",
    "ro": "RO", "rom": "RO", "romanian": "RO",
    "tr": "TR", "tur": "TR", "turkish": "TR",
    "ar": "AR", "ara": "AR", "arabic": "AR",
    "ca": "CA", "cat": "CA", "catalan": "CA",
}


def normalize_language(lang):
    return TRANSLATION_LANG_MAP.get(lang.lower(), lang.upper())


def parse_translation_paren(tag):
    lower = tag.lower()
    if lower.startswith("traducido ") or lower.startswith("traduccion "):
        return "ES"
    if lower.startswith("traduzido ") or lower == "traduzido":
        return "PT-BR"
    if lower == "pt-br":
        return "PT-BR"
    if lower.startswith("translated "):
        lang = tag[11:].strip().lower()
        return normalize_language(lang)
    return None


def parse_translation_bracket(tag):
    lower = tag.lower()
    if not (lower.startswith("t+") or lower.startswith("t-")):
        return None
    rest = lower[2:]
    # Extract lang code: until digit, space, underscore, or bracket
    lang_end = len(rest)
    for i, c in enumerate(rest):
        if c.isdigit() or c in (' ', '_', ']'):
            lang_end = i
            break
    lang = rest[:lang_end]
    if not lang:
        return None
    return normalize_language(lang)


# =============================================================================
# ROM classification (mirrors rom_tags.rs classify + extract_tags)
# =============================================================================

class RomInfo:
    """Parsed info from a ROM filename."""
    def __init__(self, filename, system):
        self.filename = filename
        self.system = system
        self.stem = filename.rsplit('.', 1)[0] if '.' in filename else filename

        self.region = None          # Normalized region string
        self.region_priority = ""   # usa/europe/japan/world/other/unknown
        self.revision = None        # "Rev 1", etc.
        self.translation = None     # "ES", "PT-BR", etc.
        self.patch_60hz = False
        self.patch_fastrom = False
        self.is_hack = False
        self.is_beta = False
        self.is_proto = False
        self.is_demo = False
        self.is_unlicensed = False
        self.is_aftermarket = False
        self.is_pirate = False
        self.hack_detail = None     # Specific hack tag text

        self._parse()

    def _parse(self):
        paren_tags = extract_paren_tags(self.stem)
        bracket_tags = extract_bracket_tags(self.stem)

        for tag in paren_tags:
            tag = tag.strip()
            if not tag:
                continue
            lower = tag.lower()

            # Revision
            rev = parse_revision(tag)
            if rev:
                self.revision = rev
                continue

            # Translation
            tl = parse_translation_paren(tag)
            if tl:
                self.translation = tl
                continue

            # Patches
            if lower == "60hz":
                self.patch_60hz = True
                continue
            if lower == "fastrom":
                self.patch_fastrom = True
                continue

            # Hack
            if (lower == "hack" or lower.endswith(" hack")
                or lower in ("smw hack", "sa-1 smw hack", "smw2 hack",
                              "smrpg hack", "smk hack", "sd gundam g next hack",
                              "uncensored hack")):
                self.is_hack = True
                self.hack_detail = tag
                continue

            # Beta / Proto / Demo
            if lower == "beta" or lower.startswith("beta "):
                self.is_beta = True
                continue
            if lower in ("proto", "prototype") or lower.startswith("proto "):
                self.is_proto = True
                continue
            if lower == "demo" or lower.startswith("demo "):
                self.is_demo = True
                continue

            # Unlicensed
            if lower in ("unl", "unlicensed"):
                self.is_unlicensed = True
                continue

            # Aftermarket / Homebrew
            if lower in ("aftermarket", "homebrew"):
                self.is_aftermarket = True
                continue

            # Pirate
            if lower == "pirate":
                self.is_pirate = True
                continue

            # Noise
            if is_noise_tag(lower):
                continue

            # Region
            if self.region is None and looks_like_region(tag):
                self.region = normalize_region(tag)
                self.region_priority = region_to_priority(tag)
                continue

        # Bracket translation tags
        for tag in bracket_tags:
            tag = tag.strip()
            if not tag:
                continue
            tl = parse_translation_bracket(tag)
            if tl and self.translation is None:
                self.translation = tl

        if self.region_priority == "":
            self.region_priority = "unknown"

    @property
    def tier(self):
        """Classification tier matching rom_tags.rs RomTier."""
        if self.is_pirate:
            return "pirate"
        if self.is_beta or self.is_proto or self.is_demo:
            return "prerelease"
        if self.is_hack:
            return "hack"
        if self.is_aftermarket:
            return "homebrew"
        if self.is_unlicensed:
            return "unlicensed"
        if self.translation:
            return "translation"
        if self.revision:
            return "revision"
        if self.region and self.region_priority == "other":
            return "region_variant"
        return "original"

    @property
    def is_modified(self):
        """Is this a regional modification (patched but keeping same region)?"""
        return bool(self.translation or self.patch_60hz or self.patch_fastrom
                     or self.revision)

    @property
    def modification_type(self):
        """Return a classification of the modification."""
        mods = []
        if self.translation:
            mods.append(f"translation:{self.translation}")
        if self.patch_60hz:
            mods.append("patch:60Hz")
        if self.patch_fastrom:
            mods.append("patch:FastROM")
        if self.revision:
            mods.append(f"revision:{self.revision}")
        if self.is_hack:
            mods.append(f"hack:{self.hack_detail or 'generic'}")
        return mods if mods else ["original"]


def base_title(name):
    """Compute lowercased base title, matching thumbnails.rs base_title()."""
    # Handle tilde dual-names
    if " ~ " in name:
        name = name.rsplit(" ~ ", 1)[1]

    # Strip tags (from first ' (' or ' [')
    for marker in (" (", " ["):
        idx = name.find(marker)
        if idx != -1:
            name = name[:idx]
            break

    name = name.strip().lower()

    # Normalize trailing articles
    for article in (", the", ", an", ", a"):
        if name.endswith(article):
            art = article[2:]  # skip ", "
            name = f"{art} {name[:-len(article)]}"
            break

    return name


# =============================================================================
# ROM scanning
# =============================================================================

def scan_roms(roms_dir):
    """Scan all ROM files from the NFS romset, grouped by system."""
    systems = {}

    if not roms_dir.exists():
        print(f"ERROR: ROM directory not found: {roms_dir}", file=sys.stderr)
        sys.exit(1)

    for entry in sorted(roms_dir.iterdir()):
        if not entry.is_dir():
            continue
        system_name = entry.name
        if any(system_name.startswith(p) for p in SKIP_PREFIXES):
            continue

        rom_files = []
        for root, dirs, files in os.walk(str(entry)):
            # Skip directories starting with _ (matching thumbnails.rs logic)
            dirs[:] = [d for d in dirs if not d.startswith('_')]
            for f in files:
                rom_files.append(f)

        if rom_files:
            systems[system_name] = rom_files

    return systems


# =============================================================================
# Analysis
# =============================================================================

def analyze_system(system_name, filenames):
    """Analyze a single system's ROMs for regional modifications."""
    # Parse all ROM info
    roms = [RomInfo(fn, system_name) for fn in filenames]

    # Group by base_title
    by_title = defaultdict(list)
    for rom in roms:
        bt = base_title(rom.stem)
        by_title[bt].append(rom)

    # For each title, group by region_priority
    regional_mod_groups = []
    for bt, group in by_title.items():
        by_region = defaultdict(list)
        for rom in group:
            by_region[rom.region_priority].append(rom)

        # Find regions with multiple ROMs
        for region, region_roms in by_region.items():
            if len(region_roms) > 1:
                # This region has multiple ROMs for the same game
                originals = [r for r in region_roms if r.tier == "original"]
                modifications = [r for r in region_roms if r.tier != "original"]
                if originals and modifications:
                    regional_mod_groups.append({
                        "base_title": bt,
                        "region": region,
                        "region_display": originals[0].region or region,
                        "originals": originals,
                        "modifications": modifications,
                        "all_roms": region_roms,
                    })

    return roms, by_title, regional_mod_groups


def classify_modification(rom):
    """Classify a modification ROM into a category."""
    if rom.translation:
        return "translation"
    if rom.patch_fastrom:
        return "fastrom"
    if rom.patch_60hz:
        return "60hz"
    if rom.revision:
        return "revision"
    if rom.is_hack:
        return "hack"
    if rom.is_aftermarket:
        return "homebrew"
    if rom.is_unlicensed:
        return "unlicensed"
    if rom.is_beta or rom.is_proto or rom.is_demo:
        return "prerelease"
    return "other"


def main():
    print("=" * 80)
    print("Regional Modifications Analysis")
    print(f"ROM directory: {NFS_ROMS}")
    print("=" * 80)
    print()

    systems_data = scan_roms(NFS_ROMS)

    total_roms = 0
    total_titles = 0
    total_mod_groups = 0
    total_mod_roms = 0
    global_mod_types = defaultdict(int)
    all_examples = []

    system_stats = []

    for system_name in sorted(systems_data.keys()):
        filenames = systems_data[system_name]
        roms, by_title, mod_groups = analyze_system(system_name, filenames)

        total_roms += len(roms)
        total_titles += len(by_title)
        total_mod_groups += len(mod_groups)

        # Count mod types for this system
        system_mod_types = defaultdict(int)
        system_mod_roms = 0
        for group in mod_groups:
            for rom in group["modifications"]:
                mod_type = classify_modification(rom)
                system_mod_types[mod_type] += 1
                global_mod_types[mod_type] += 1
                system_mod_roms += 1
                total_mod_roms += 1

        # Count tiers
        tier_counts = defaultdict(int)
        for rom in roms:
            tier_counts[rom.tier] += 1

        system_stats.append({
            "system": system_name,
            "total_roms": len(roms),
            "unique_titles": len(by_title),
            "mod_groups": len(mod_groups),
            "mod_roms": system_mod_roms,
            "mod_types": dict(system_mod_types),
            "tier_counts": dict(tier_counts),
        })

        # Collect examples (up to 3 per system)
        for group in mod_groups[:5]:
            all_examples.append({
                "system": system_name,
                "base_title": group["base_title"],
                "region": group["region"],
                "region_display": group["region_display"],
                "originals": [r.filename for r in group["originals"]],
                "modifications": [(r.filename, classify_modification(r),
                                    r.modification_type) for r in group["modifications"]],
            })

    # =============================================================================
    # Output: Per-system stats
    # =============================================================================

    print("PER-SYSTEM STATISTICS")
    print("-" * 80)
    print(f"{'System':<25} {'ROMs':>7} {'Titles':>7} {'Mod Groups':>11} {'Mod ROMs':>9}")
    print("-" * 80)

    for s in sorted(system_stats, key=lambda x: x["mod_groups"], reverse=True):
        print(f"{s['system']:<25} {s['total_roms']:>7} {s['unique_titles']:>7} "
              f"{s['mod_groups']:>11} {s['mod_roms']:>9}")

    print("-" * 80)
    print(f"{'TOTAL':<25} {total_roms:>7} {total_titles:>7} "
          f"{total_mod_groups:>11} {total_mod_roms:>9}")
    print()

    # =============================================================================
    # Output: Tier breakdown
    # =============================================================================

    print("ROM TIER BREAKDOWN (across all systems)")
    print("-" * 60)
    all_tiers = defaultdict(int)
    for s in system_stats:
        for tier, count in s["tier_counts"].items():
            all_tiers[tier] += count
    for tier in ["original", "revision", "region_variant", "translation",
                 "unlicensed", "homebrew", "hack", "prerelease", "pirate"]:
        if tier in all_tiers:
            pct = 100 * all_tiers[tier] / total_roms if total_roms else 0
            print(f"  {tier:<20} {all_tiers[tier]:>7}  ({pct:>5.1f}%)")
    print()

    # =============================================================================
    # Output: Modification types
    # =============================================================================

    print("MODIFICATION TYPES (ROMs that share base_title + region with an original)")
    print("-" * 60)
    for mod_type, count in sorted(global_mod_types.items(), key=lambda x: -x[1]):
        pct = 100 * count / total_mod_roms if total_mod_roms else 0
        print(f"  {mod_type:<20} {count:>7}  ({pct:>5.1f}%)")
    print(f"  {'TOTAL':<20} {total_mod_roms:>7}")
    print()

    # =============================================================================
    # Output: Systems with most modifications
    # =============================================================================

    print("TOP SYSTEMS BY MODIFICATION COUNT")
    print("-" * 60)
    for s in sorted(system_stats, key=lambda x: x["mod_roms"], reverse=True)[:15]:
        if s["mod_roms"] == 0:
            break
        types_str = ", ".join(f"{k}={v}" for k, v in sorted(s["mod_types"].items(), key=lambda x: -x[1]))
        print(f"  {s['system']:<25} {s['mod_roms']:>5} mods  ({types_str})")
    print()

    # =============================================================================
    # Output: Examples
    # =============================================================================

    print("EXAMPLES OF REGIONAL MODIFICATIONS")
    print("(games where multiple ROMs share the same base_title AND region)")
    print("-" * 80)

    # Group examples by modification type for better readability
    by_mod_type = defaultdict(list)
    for ex in all_examples:
        for fn, mod_type, mod_detail in ex["modifications"]:
            by_mod_type[mod_type].append(ex)
            break  # Just use first mod to categorize

    for mod_type in ["translation", "fastrom", "revision", "hack", "60hz", "other"]:
        examples = by_mod_type.get(mod_type, [])
        if not examples:
            continue
        print(f"\n  === {mod_type.upper()} ===")
        seen = set()
        count = 0
        for ex in examples:
            key = (ex["system"], ex["base_title"], ex["region"])
            if key in seen:
                continue
            seen.add(key)
            count += 1
            if count > 8:
                remaining = len([e for e in examples
                                 if (e["system"], e["base_title"], e["region"]) not in seen])
                if remaining > 0:
                    print(f"    ... and {remaining} more")
                break

            print(f"\n  [{ex['system']}] \"{ex['base_title']}\" region={ex['region_display']}")
            print(f"    Originals:")
            for fn in ex["originals"]:
                print(f"      - {fn}")
            print(f"    Modifications:")
            for fn, mt, detail in ex["modifications"]:
                print(f"      - {fn}")
                print(f"        type={mt}, detail={detail}")

    print()

    # =============================================================================
    # Output: Dedup impact analysis
    # =============================================================================

    print("=" * 80)
    print("DEDUP IMPACT ANALYSIS")
    print("=" * 80)
    print()
    print("The dedup CTE in recommendations uses:")
    print("  PARTITION BY system, base_title")
    print("  ORDER BY CASE WHEN region = <pref> THEN 0 ... END")
    print("  WHERE is_translation = 0 AND is_hack = 0")
    print()
    print("This means:")
    print("  - Translations and hacks are already excluded from dedup")
    print("  - Revisions (Rev 1, etc.) compete with originals in the same partition")
    print("  - FastROM patches are NOT flagged as translations or hacks,")
    print("    so they compete with originals in the same partition")
    print("  - 60Hz patches are NOT flagged as translations or hacks,")
    print("    so they compete with originals in the same partition")
    print()

    # Count FastROM and 60Hz ROMs specifically
    fastrom_count = 0
    hz60_count = 0
    revision_count = 0
    for s in system_stats:
        for system_name in systems_data:
            pass
    # Re-scan for specific counts
    for system_name in sorted(systems_data.keys()):
        filenames = systems_data[system_name]
        for fn in filenames:
            rom = RomInfo(fn, system_name)
            if rom.patch_fastrom and rom.tier == "original":
                fastrom_count += 1
            if rom.patch_60hz and rom.tier == "original":
                hz60_count += 1
            if rom.revision and rom.tier == "revision":
                revision_count += 1

    print(f"  FastROM patches (tier=original, compete in dedup): {fastrom_count}")
    print(f"  60Hz patches (tier=original, compete in dedup): {hz60_count}")
    print(f"  Revisions (tier=revision, compete in dedup): {revision_count}")
    print()
    print("  Note: FastROM and 60Hz patches are classified as 'original' tier")
    print("  because rom_tags.rs classify() does not check for them.")
    print("  They only appear in extract_tags() for display purposes.")
    print("  This means in the dedup partition, they compete with clean originals")
    print("  and the winner depends solely on region priority.")
    print()

    # =============================================================================
    # Output: Regional variants impact
    # =============================================================================

    print("REGIONAL VARIANTS QUERY IMPACT")
    print("-" * 60)
    print()
    print("The regional_variants() query returns ROMs WHERE:")
    print("  - same system, same base_title")
    print("  - is_translation = 0 AND is_hack = 0")
    print()
    print("This means FastROM patches and 60Hz patches WILL appear")
    print("as regional variants alongside the clean original.")
    print("Revisions also appear as variants.")
    print()
    print("Example: If we have:")
    print("  'ActRaiser (USA).sfc' and 'Actraiser (USA) (FastRom).sfc'")
    print("  Both have region='usa', is_translation=0, is_hack=0")
    print("  Both would appear in regional variants with label 'usa'")
    print("  => Two chips both labeled 'usa', confusing for the user")
    print()

    # Count how many variant groups would show duplicate region labels
    dup_label_groups = 0
    for system_name in sorted(systems_data.keys()):
        filenames = systems_data[system_name]
        roms = [RomInfo(fn, system_name) for fn in filenames]
        by_title = defaultdict(list)
        for rom in roms:
            bt = base_title(rom.stem)
            by_title[bt].append(rom)

        for bt, group in by_title.items():
            # Filter like regional_variants() does: no translations, no hacks
            variants = [r for r in group if not r.translation and not r.is_hack]
            # Check for duplicate regions
            region_counts = defaultdict(int)
            for r in variants:
                if r.region_priority != "unknown":
                    region_counts[r.region_priority] += 1
            if any(c > 1 for c in region_counts.values()):
                dup_label_groups += 1

    print(f"  Games with duplicate region labels in variants: {dup_label_groups}")
    print()


if __name__ == "__main__":
    main()
