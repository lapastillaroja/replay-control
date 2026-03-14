#!/usr/bin/env python3
"""
Merge catver.ini and category.ini into a single catver-format file.

catver.ini uses: romname=Category (under [Category] section)
category.ini uses: [Category Name] with rom names listed underneath

The merge takes all entries from catver.ini as the baseline, then adds any
entries from category.ini that aren't already present. This ensures we get
the broadest category coverage across MAME versions.

Usage: merge-catver.py <catver.ini> <category.ini> <output.ini>
"""

import sys


def parse_catver(path):
    """Parse catver.ini: romname=category under [Category] section."""
    categories = {}
    veradded = {}
    in_category = False
    in_veradded = False

    with open(path, encoding="utf-8", errors="replace") as f:
        for line in f:
            line = line.strip()
            if not line or line.startswith(";"):
                continue
            if line.startswith("["):
                section = line[1:].rstrip("]").strip()
                in_category = section == "Category"
                in_veradded = section == "VerAdded"
                continue
            if "=" in line:
                rom, _, value = line.partition("=")
                rom = rom.strip()
                value = value.strip()
                if rom and value:
                    if in_category:
                        categories[rom] = value
                    elif in_veradded:
                        veradded[rom] = value

    return categories, veradded


def parse_category_ini(path):
    """Parse category.ini: [Category Name] with rom names listed below."""
    categories = {}
    current_category = None

    with open(path, encoding="utf-8", errors="replace") as f:
        for line in f:
            line = line.strip()
            if not line or line.startswith(";"):
                continue
            if line.startswith("["):
                section = line[1:].rstrip("]").strip()
                if section in ("FOLDER_SETTINGS", "ROOT_FOLDER"):
                    current_category = None
                else:
                    current_category = section
                continue
            if current_category and line:
                categories[line] = current_category

    return categories


def main():
    if len(sys.argv) != 4:
        print(f"Usage: {sys.argv[0]} <catver.ini> <category.ini> <output.ini>")
        sys.exit(1)

    catver_path, category_path, output_path = sys.argv[1], sys.argv[2], sys.argv[3]

    # Parse both sources
    categories, veradded = parse_catver(catver_path)
    base_count = len(categories)

    category_ini = parse_category_ini(category_path)

    # Merge: add category.ini entries not already in catver
    added = 0
    for rom, cat in category_ini.items():
        if rom not in categories:
            categories[rom] = cat
            added += 1

    print(f"  catver.ini base entries: {base_count}")
    print(f"  category.ini entries: {len(category_ini)}")
    print(f"  New entries from category.ini: {added}")
    print(f"  Total merged entries: {len(categories)}")

    # Detect version from category.ini header
    version = "unknown"
    with open(category_path, encoding="utf-8", errors="replace") as f:
        for line in f:
            if line.strip().startswith(";;") and "CATEGORY.ini" in line:
                # e.g. ";; CATEGORY.ini 0.285 / 10-Feb-26 / MAME 0.285 ;;"
                version = line.strip().strip(";").strip()
                break

    # Write merged output in catver.ini format
    with open(output_path, "w", encoding="utf-8") as f:
        f.write("[FOLDER_SETTINGS]\n")
        f.write("RootFolderIcon mame\n")
        f.write("SubFolderIcon folder\n")
        f.write("\n")
        f.write(f";; merged catver / {version} ;;\n")
        f.write("\n")
        f.write("[ROOT_FOLDER]\n")
        f.write("\n")
        f.write("[Category]\n")
        for rom_name in sorted(categories.keys()):
            f.write(f"{rom_name}={categories[rom_name]}\n")

        if veradded:
            f.write("\n[VerAdded]\n")
            for rom_name in sorted(veradded.keys()):
                f.write(f"{rom_name}={veradded[rom_name]}\n")

    print(f"  Written to {output_path}")


if __name__ == "__main__":
    main()
