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
    ["game-series"]=5 ["recommendations"]=6 ["metadata"]=7 ["thumbnails"]=8
    ["configuration"]=9 ["settings"]=10 ["storage"]=11 ["benchmarks"]=12
    ["libretro-core"]=13
)

for f in "$REPO_ROOT/docs/features/"*.md; do
    base=$(basename "$f" .md)
    [[ "$base" == "index" ]] && base="overview"
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
    ["technical-foundation"]=1 ["design-decisions"]=2 ["startup-pipeline"]=3
    ["database-schema"]=4 ["server-functions"]=5 ["connection-pooling"]=6
    ["enrichment"]=7 ["rom-classification"]=8 ["activity-system"]=9
)

for f in "$REPO_ROOT/docs/architecture/"*.md; do
    base=$(basename "$f" .md)
    [[ "$base" == "index" ]] && base="overview"
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
