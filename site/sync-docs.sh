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
find "$FEATURES_DIR" -maxdepth 1 -type f -name '*.md' ! -name '_index.md' -delete

cat > "$FEATURES_DIR/_index.md" << EOF
---
title: "Features"
description: "Replay Control feature documentation."
weight: 10
toc: true
layout: single
---

EOF
tail -n +2 "$REPO_ROOT/docs/features/index.md" >> "$FEATURES_DIR/_index.md"
echo "  features/_index.md"

declare -A FEATURE_WEIGHTS=(
    ["getting-started"]=1 ["install"]=2 ["game-library"]=3 ["favorites"]=4
    ["recents"]=5 ["now-playing"]=6 ["search"]=7 ["game-detail"]=8
    ["arcade-boards"]=9 ["game-series"]=10 ["recommendations"]=11
    ["library-management"]=12 ["thumbnails"]=13 ["configuration"]=14
    ["settings"]=15 ["updates"]=16 ["storage"]=17 ["benchmarks"]=18
    ["libretro-core"]=19
)

for f in "$REPO_ROOT/docs/features/"*.md; do
    base=$(basename "$f" .md)
    [[ "$base" == "index" ]] && continue  # _index.md handles the section page
    weight="${FEATURE_WEIGHTS[$base]:-50}"
    title=$(head -1 "$f" | sed 's/^# //')
    
    target="$FEATURES_DIR/$base.md"
    slug_line=""
    if [[ "$base" == "library-management" ]]; then
        slug_line='slug: "library-management"'
    fi
    cat > "$target" << EOF
---
title: "$title"
date: 2025-01-01T00:00:00+00:00
lastmod: 2025-01-01T00:00:00+00:00
draft: false
weight: $weight
toc: true
$slug_line
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
    ["library-build-pipeline"]=104 ["database-schema"]=105 ["server-functions"]=106
    ["connection-pooling"]=107 ["enrichment"]=108 ["arcade-boards"]=109
    ["rom-classification"]=110 ["activity-system"]=111
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

# Contributing docs
CONTRIB_DIR="$CONTENT/contributing"
if [[ -d "$REPO_ROOT/docs/contributing" ]]; then
    mkdir -p "$CONTRIB_DIR"

    declare -A CONTRIB_WEIGHTS=(
        ["community-metadata"]=201
    )

    for f in "$REPO_ROOT/docs/contributing/"*.md; do
        base=$(basename "$f" .md)
        [[ "$base" == "index" ]] && continue
        weight="${CONTRIB_WEIGHTS[$base]:-250}"
        title=$(head -1 "$f" | sed 's/^# //')

        target="$CONTRIB_DIR/$base.md"
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
        echo "  contributing/$base.md (weight $weight)"
    done
fi

echo "Done!"
