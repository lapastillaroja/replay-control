#!/usr/bin/env python3
"""
Extract arcade game metadata from the full MAME listxml into a compact XML file.

Filters out non-arcade entries (BIOS, devices, mechanical, non-runnable) and strips
ROM/disk/chip details, keeping only the metadata fields needed by replay-core's
build.rs: name, description, year, manufacturer, cloneof, rotation, players, and
driver status.

Usage: python3 extract-mame-arcade.py <input.xml> <output.xml>

Input:  Full MAME listxml (~285 MB, ~49K machines)
Output: Compact arcade-only XML (~3.6 MB, ~15K-27K machines)
"""

import html
import sys
import xml.etree.ElementTree as ET


def main():
    if len(sys.argv) != 3:
        print(f"Usage: {sys.argv[0]} <input.xml> <output.xml>", file=sys.stderr)
        sys.exit(1)

    input_path = sys.argv[1]
    output_path = sys.argv[2]

    lines = []
    lines.append('<?xml version="1.0"?>')
    lines.append('<mame version="0.285">')

    arcade_count = 0
    total_count = 0

    for event, elem in ET.iterparse(input_path, events=("end",)):
        if elem.tag != "machine":
            continue

        total_count += 1

        # Filter out non-arcade entries
        if elem.get("isbios", "no") == "yes":
            elem.clear()
            continue
        if elem.get("isdevice", "no") == "yes":
            elem.clear()
            continue
        if elem.get("ismechanical", "no") == "yes":
            elem.clear()
            continue
        if elem.get("runnable", "yes") == "no":
            elem.clear()
            continue

        name = elem.get("name", "")
        cloneof = elem.get("cloneof", "")

        desc_el = elem.find("description")
        description = desc_el.text if desc_el is not None and desc_el.text else ""

        year_el = elem.find("year")
        year = year_el.text if year_el is not None and year_el.text else ""

        mfr_el = elem.find("manufacturer")
        manufacturer = mfr_el.text if mfr_el is not None and mfr_el.text else ""

        # Get rotation from first display element
        display_el = elem.find("display")
        rotate = display_el.get("rotate", "") if display_el is not None else ""

        # Get players from input element
        input_el = elem.find("input")
        players = input_el.get("players", "0") if input_el is not None else "0"

        # Get driver status
        driver_el = elem.find("driver")
        status = driver_el.get("status", "unknown") if driver_el is not None else "unknown"

        # Escape XML special chars in text content
        description = html.escape(description)
        manufacturer = html.escape(manufacturer)

        # Build compact element with attributes for metadata
        attrs = f'name="{name}"'
        if cloneof:
            attrs += f' cloneof="{cloneof}"'
        if rotate:
            attrs += f' rotate="{rotate}"'
        if players != "0":
            attrs += f' players="{players}"'
        if status != "unknown":
            attrs += f' status="{status}"'

        lines.append(
            f"<m {attrs}><d>{description}</d><y>{year}</y><f>{manufacturer}</f></m>"
        )

        arcade_count += 1
        elem.clear()

    lines.append("</mame>")

    with open(output_path, "w", encoding="utf-8") as f:
        f.write("\n".join(lines))

    print(f"  Processed {total_count} machines, extracted {arcade_count} arcade entries")


if __name__ == "__main__":
    main()
