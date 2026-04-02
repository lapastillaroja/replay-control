#!/bin/bash
#
# filter-research.sh -- Remove research/ directory from git history before publication.
#
# Creates a fresh clone of the source repo at a destination path, then runs
# git filter-repo to strip all research/ files from every commit. The original
# repo is never modified.
#
# Prerequisites:
#   - git filter-repo (https://github.com/newren/git-filter-repo)
#
# Usage:
#   ./scripts/filter-research.sh <source-repo> <destination-path>
#
# Example:
#   ./scripts/filter-research.sh ~/workspace/replay-control /tmp/replay-control-filtered
#
set -euo pipefail

# --- Argument parsing ---

if [[ $# -ne 2 ]]; then
    echo "Usage: $0 <source-repo> <destination-path>"
    echo ""
    echo "Creates a fresh clone of <source-repo> at <destination-path> and removes"
    echo "the research/ directory from the entire git history."
    echo ""
    echo "The source repo is NEVER modified."
    exit 1
fi

SOURCE="$(realpath "$1")"
DEST="$2"

if [[ ! -d "$SOURCE/.git" ]]; then
    echo "ERROR: $SOURCE is not a git repository"
    exit 1
fi

if [[ -e "$DEST" ]]; then
    echo "ERROR: $DEST already exists. Remove it first or choose a different path."
    exit 1
fi

if ! command -v git-filter-repo &>/dev/null; then
    echo "ERROR: git-filter-repo is not installed."
    echo "Install: pip install git-filter-repo"
    exit 1
fi

# --- Phase 1: Clone ---

echo "=== Phase 1: Clone source repo ==="
echo "  Source:      $SOURCE"
echo "  Destination: $DEST"
git clone --no-hardlinks "$SOURCE" "$DEST"
cd "$DEST"

COMMITS_BEFORE=$(git rev-list --all --count)
echo "  Commits: $COMMITS_BEFORE"
echo ""

# --- Phase 2: Remove research/ from history ---

echo "=== Phase 2: Remove research/ from history ==="
echo "  Removing: research/ (all subdirectories and files)"

git filter-repo \
    --path research/ \
    --invert-paths \
    --force

COMMITS_AFTER=$(git rev-list --all --count)
echo ""
echo "  Commits before: $COMMITS_BEFORE"
echo "  Commits after:  $COMMITS_AFTER"
echo "  Removed:        $((COMMITS_BEFORE - COMMITS_AFTER)) empty commits"
echo ""

# --- Phase 3: Rewrite commit messages referencing research/ paths ---

echo "=== Phase 3: Clean commit messages ==="

git filter-repo --message-callback '
import re
msg = message
# Replace specific research/ path references in commit messages
replacements = [
    # Specific path references
    (b"research/investigations/", b""),
    (b"research/plans/", b""),
    (b"research/reference/", b""),
    (b"research/design/", b""),
    (b"research/private/", b""),
    # General research/ references
    (b"research/ (internal)", b"internal docs"),
    (b"move internal files to research", b"reorganize internal docs"),
    (b"and research/ (internal)", b""),
    (b"plus a separate research/ directory at the repository root for internal development work", b""),
    (b"research/", b""),
    (b"(public) and  (internal)", b""),
]
for old, new in replacements:
    msg = msg.replace(old, new)
# Collapse multiple spaces from replacements
msg = re.sub(b"  +", b" ", msg)
# Remove lines that became empty or whitespace-only after replacements
lines = msg.split(b"\n")
cleaned = []
for line in lines:
    stripped = line.strip()
    if stripped == b"" or stripped == b"-" or stripped == b"- **":
        # Keep truly blank lines (paragraph separators) but skip lines that became just a dash
        if stripped == b"":
            cleaned.append(line)
        continue
    cleaned.append(line)
msg = b"\n".join(cleaned)
return msg
' --force

echo "  Done."
echo ""

# --- Phase 4: Check for broken references in docs ---

echo "=== Phase 4: Check for broken research/ references ==="

BROKEN_REFS=0

# Check working tree for remaining research/ references in docs
if git grep -l 'research/' -- '*.md' '*.sh' '*.rs' '*.toml' '*.html' 2>/dev/null; then
    echo ""
    echo "  WARNING: The following files still reference research/ paths:"
    git grep -n 'research/' -- '*.md' '*.sh' '*.rs' '*.toml' '*.html' 2>/dev/null || true
    BROKEN_REFS=1
else
    echo "  No broken research/ references in tracked files."
fi

echo ""

# --- Phase 5: Verify history is clean ---

echo "=== Phase 5: Verify history is clean ==="

ISSUES=0

# Check that no research/ files remain in any commit
echo -n "  Checking file paths in history... "
if git log --all --diff-filter=A --name-only --pretty=format: | grep -q '^research/'; then
    echo "FAIL"
    echo "  ERROR: research/ files still appear in history!"
    echo "  Files found:"
    git log --all --diff-filter=A --name-only --pretty=format: | grep '^research/' | sort -u | head -20
    ISSUES=1
else
    echo "CLEAN"
fi

# Check commit messages for research/ references
echo -n "  Checking commit messages... "
RESEARCH_MSGS=$(git log --all --format='%H %s' | grep -i 'research/' || true)
if [[ -n "$RESEARCH_MSGS" ]]; then
    echo "WARNING"
    echo "  These commits still mention research/ in their subject line:"
    echo "$RESEARCH_MSGS" | while read -r line; do
        echo "    $line"
    done
    ISSUES=1
else
    echo "CLEAN"
fi

# Check for load test raw data files anywhere in history
echo -n "  Checking for load test data in history... "
if git log --all --diff-filter=A --name-only --pretty=format: | grep -q 'load-test-raw'; then
    echo "WARNING"
    echo "  Load test raw data files found in history (expected inside research/):"
    git log --all --diff-filter=A --name-only --pretty=format: | grep 'load-test-raw' | sort -u | head -10
else
    echo "CLEAN"
fi

echo ""

# --- Summary ---

echo "=== Summary ==="
echo "  Source:          $SOURCE (unchanged)"
echo "  Filtered clone:  $DEST"
echo "  Commits:         $COMMITS_AFTER (was $COMMITS_BEFORE)"
echo ""

if [[ $BROKEN_REFS -eq 1 ]]; then
    echo "  ACTION NEEDED: Some files reference research/ paths that no longer exist."
    echo "  These should be cleaned up before publishing. Files to review:"
    echo "    - docs/README.md (research/ section and links)"
    echo "    - docs/features/benchmarks.md (load test data references)"
    echo "    - docs/features/metadata.md (arcade-db-design.md reference)"
    echo "    - docs/features/thumbnails.md (box-art-swap.md reference)"
    echo "    - build.sh, dev.sh (cross-compilation.md reference)"
    echo "    - tools/load-test.sh (RESULTS_DIR to research/investigations/load-tests)"
    echo ""
    echo "  After fixing, commit and verify with:"
    echo "    cd $DEST"
    echo "    git grep 'research/'"
    echo ""
fi

if [[ $ISSUES -gt 0 ]]; then
    echo "  WARNING: Some verification checks failed. Review the output above."
    exit 1
else
    echo "  All checks passed."
fi
