#!/usr/bin/env bash
#
# Fetch TGDB developer/publisher/genre name lookup tables from the API.
# Requires TGDB_API_KEY environment variable.
# These are static lists (~3 API calls), well within the free 1000/month tier.
#
# Usage: TGDB_API_KEY=your_key ./scripts/download-tgdb-lookups.sh
#
# The resulting JSON files (~400KB total) land in data/upstream/ — which is
# gitignored and regenerated per build, NOT committed. They map TGDB numeric
# developer/publisher/genre ids (embedded in thegamesdb-latest.json) to names;
# without them a build has empty developer/publisher fields. build-catalog's
# preflight requires them, so a full build needs this key (CI uses the
# TGDB_API_KEY secret); pass build-catalog --allow-partial for a keyless build.

set -euo pipefail

# Source scripts/.env if it exists (for local development) — the single local
# secrets file, alongside this script and scripts/.env.example (also holds
# RETROACHIEVEMENTS_KEY, read by retroachievements-gamelist-extract.py).
SCRIPT_DIR_ENV="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [[ -f "$SCRIPT_DIR_ENV/.env" ]]; then
    set -a
    source "$SCRIPT_DIR_ENV/.env"
    set +a
fi

if [[ -z "${TGDB_API_KEY:-}" ]]; then
    echo "ERROR: Set TGDB_API_KEY environment variable" >&2
    echo "  Option 1: Add TGDB_API_KEY=your_key to scripts/.env" >&2
    echo "  Option 2: TGDB_API_KEY=your_key ./scripts/download-tgdb-lookups.sh" >&2
    echo "  Get a free key at https://api.thegamesdb.net/key.php" >&2
    exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Downloaded third-party inputs live under data/upstream (regenerable,
# gitignored) — kept separate from committed curated data so build caches that
# restore this dir can never clobber source-of-truth files.
DATA_DIR="$SCRIPT_DIR/../data/upstream"
API_BASE="https://api.thegamesdb.net/v1"

mkdir -p "$DATA_DIR"

for endpoint in Developers Publishers Genres; do
    lower=$(echo "$endpoint" | tr 'A-Z' 'a-z')
    dest="$DATA_DIR/tgdb-${lower}.json"
    echo "Fetching /v1/$endpoint..."

    response=$(curl -fSL --retry 3 --retry-delay 2 "$API_BASE/$endpoint?apikey=$TGDB_API_KEY") || {
        echo "  FAILED to fetch /v1/$endpoint" >&2
        exit 1
    }

    echo "$response" | python3 -c "
import sys, json
raw = json.load(sys.stdin)
data = raw.get('data', {}).get('${lower}', {})
# Convert to simple {id: name} map
result = {k: v['name'] for k, v in data.items()}
json.dump(result, sys.stdout, indent=2, ensure_ascii=False, sort_keys=True)
print()
" > "$dest"

    count=$(python3 -c "import json; print(len(json.load(open('$dest'))))")
    echo "  Saved $count entries to $dest"
done

echo
echo "Done. These land in data/upstream/ (gitignored) — re-fetched per build, not committed."
echo "Re-run periodically to pick up new developers/publishers (rarely changes)."
