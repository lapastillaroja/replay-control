#!/bin/bash
# Sync docs from the main repo docs/ to the Doks content directory.
# Adds Doks frontmatter (title, weight) to each file.
# Run from the site/ directory: bash sync-docs.sh

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
CONTENT="$SCRIPT_DIR/content/docs"

echo "Syncing docs from $REPO_ROOT/docs/ ..."

# Feature docs
FEATURES_DIR="$CONTENT/features"
mkdir -p "$FEATURES_DIR"

declare -A FEATURE_WEIGHTS=(
    ["getting-started"]=1 ["install"]=2 ["game-library"]=3 ["search"]=4
    ["game-detail"]=5 ["game-series"]=6 ["recommendations"]=7 ["metadata"]=8 ["thumbnails"]=9
    ["configuration"]=10 ["settings"]=11 ["updates"]=12 ["storage"]=13 ["benchmarks"]=14
    ["libretro-core"]=15
)

for f in "$REPO_ROOT/docs/features/"*.md; do
    base=$(basename "$f" .md)
    [[ "$base" == "index" ]] && continue  # _index.md handles the section page
    weight="${FEATURE_WEIGHTS[$base]:-50}"
    title=$(head -1 "$f" | sed 's/^# //')
    
    target="$FEATURES_DIR/$base.md"
    cat > "$target" << EOF
---
title: "$title"
date: 2025-01-01T00:00:00+00:00
lastmod: 2025-01-01T00:00:00+00:00
draft: false
weight: $weight
toc: true
---

EOF
    # Append content after the first heading
    tail -n +2 "$f" >> "$target"
    echo "  features/$base.md (weight $weight)"
done

# Architecture docs
ARCH_DIR="$CONTENT/architecture"
mkdir -p "$ARCH_DIR"

declare -A ARCH_WEIGHTS=(
    ["technical-foundation"]=101 ["design-decisions"]=102 ["startup-pipeline"]=103
    ["database-schema"]=104 ["server-functions"]=105 ["connection-pooling"]=106
    ["enrichment"]=107 ["rom-classification"]=108 ["activity-system"]=109
)

for f in "$REPO_ROOT/docs/architecture/"*.md; do
    base=$(basename "$f" .md)
    [[ "$base" == "index" ]] && continue  # _index.md handles the section page
    weight="${ARCH_WEIGHTS[$base]:-50}"
    title=$(head -1 "$f" | sed 's/^# //')
    
    target="$ARCH_DIR/$base.md"
    cat > "$target" << EOF
---
title: "$title"
date: 2025-01-01T00:00:00+00:00
lastmod: 2025-01-01T00:00:00+00:00
draft: false
weight: $weight
toc: true
---

EOF
    tail -n +2 "$f" >> "$target"
    echo "  architecture/$base.md (weight $weight)"
done

echo "Done!"
