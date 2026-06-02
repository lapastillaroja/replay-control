#!/usr/bin/env python3
"""Validate a Shmups Wiki snapshot (data/shmups-wiki/games.json).

Run by the refresh-shmups-wiki GitHub workflow and usable locally:

    python3 scripts/validate-shmups-wiki.py [path/to/games.json]

Exits non-zero with a message on the first structural problem; prints the row
count on success.
"""

import json
import sys
from pathlib import Path

# normalized_title + page_title are required; video_index (bool flag),
# video_index_inherits_from (parent page title) and video_index_anchor (section
# anchor on the parent's Video Index) are optional. A page either has its own
# Video Index or inherits a parent's, never both; an anchor only makes sense
# alongside an inherited parent. Mirrors ShmupsWikiBuildEntry and the
# if/else-if in build-catalog's load_shmups_wiki_resources.
REQUIRED = {"normalized_title", "page_title"}
OPTIONAL = {"video_index", "video_index_inherits_from", "video_index_anchor"}
ALLOWED = REQUIRED | OPTIONAL

DEFAULT_PATH = Path("data/shmups-wiki/games.json")


def validate(path: Path) -> int:
    rows = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(rows, list):
        raise SystemExit("snapshot must be a JSON array")
    # Reject an empty snapshot: a degraded wiki response can make the extract
    # emit [], which would silently strip every shmups link from the catalog.
    if not rows:
        raise SystemExit("snapshot is empty")
    if rows:
        bad_shape = [
            idx
            for idx, row in enumerate(rows)
            if not isinstance(row, dict)
            or not REQUIRED <= set(row)
            or not set(row) <= ALLOWED
        ]
        if bad_shape:
            raise SystemExit(f"rows with unexpected shape: {bad_shape[:10]}")
        empty_keys = [
            idx
            for idx, row in enumerate(rows)
            if not row["normalized_title"] or not row["page_title"]
        ]
        if empty_keys:
            raise SystemExit(f"rows with empty keys: {empty_keys[:10]}")
        bad_optional = [
            idx
            for idx, row in enumerate(rows)
            if ("video_index" in row and not isinstance(row["video_index"], bool))
            or (
                "video_index_inherits_from" in row
                and not (
                    isinstance(row["video_index_inherits_from"], str)
                    and row["video_index_inherits_from"]
                )
            )
            or (row.get("video_index") and "video_index_inherits_from" in row)
            or (
                "video_index_anchor" in row
                and not (
                    isinstance(row["video_index_anchor"], str)
                    and row["video_index_anchor"]
                )
            )
            or (
                "video_index_anchor" in row
                and "video_index_inherits_from" not in row
            )
        ]
        if bad_optional:
            raise SystemExit(f"rows with invalid video-index fields: {bad_optional[:10]}")
        keys = [row["normalized_title"] for row in rows]
        if len(keys) != len(set(keys)):
            raise SystemExit("snapshot contains duplicate normalized_title keys")
        if keys != sorted(keys):
            raise SystemExit("snapshot must be sorted by normalized_title")
    print(f"validated {len(rows)} Shmups Wiki rows")
    return 0


if __name__ == "__main__":
    target = Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_PATH
    sys.exit(validate(target))
