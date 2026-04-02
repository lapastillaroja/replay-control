#!/usr/bin/env bash
#
# Count Rust lines of code, separating production from inline test code.
# Usage: ./tools/count-lines.sh

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

CRATES=(
  "replay-control-core/src"
  "replay-control-app/src"
)

# Format a number with thousands separators
fmt() {
  printf "%'d" "$1"
}

# Right-align a formatted number to a given width
rfmt() {
  local width=$1 num=$2
  printf "%${width}s" "$(fmt "$num")"
}

# Count lines in a set of .rs files using awk.
# Outputs: files prod test comments blanks total
count_lines() {
  local dir="$1"
  local files
  files=$(find "$dir" -name '*.rs' -not -path '*/tests/*' -not -path '*/target/*' | sort)

  if [[ -z "$files" ]]; then
    echo "0 0 0 0 0 0"
    return
  fi

  local nfiles
  nfiles=$(echo "$files" | wc -l)

  echo "$files" | xargs awk '
    BEGIN {
      in_test = 0
      test_brace_depth = 0
      in_block_comment = 0
      prod = 0; test = 0; comments = 0; blanks = 0; total = 0
    }

    # Reset per-file state.
    FNR == 1 {
      in_block_comment = 0
      in_test = 0
      test_brace_depth = 0
    }

    # Strip string and char literals so /* */ { } inside them are ignored.
    function strip_strings(s) {
      gsub(/"([^"\\]|\\.)*"/, "", s)
      gsub(/'"'"'[^'"'"'\\]'"'"'/, "", s)
      gsub(/'"'"'\\.[^'"'"']*'"'"'/, "", s)
      return s
    }

    # Check if a line (after stripping strings) opens a block comment.
    # Returns: 0 = no block comment, 1 = block comment opened (not closed),
    #          2 = block comment opened and closed on same line (pure comment),
    #          3 = block comment opened and closed but code remains on line.
    function check_block_comment(stripped) {
      if (stripped ~ /\/\*/) {
        if (stripped ~ /\*\//) {
          tmp = stripped
          gsub(/\/\*([^*]|\*[^\/])*\*\//, "", tmp)
          gsub(/[ \t]/, "", tmp)
          if (tmp == "") return 2
          return 3
        }
        return 1
      }
      return 0
    }

    {
      total++
      line = $0
      stripped = strip_strings(line)

      # --- Inside a test block: everything counts as test ---
      if (in_test) {
        # Still need to track block comments for brace counting.
        if (in_block_comment) {
          if (stripped ~ /\*\//) in_block_comment = 0
          test++
          next
        }
        bc = check_block_comment(stripped)
        if (bc == 1) { in_block_comment = 1; test++; next }

        # Track brace depth (on non-block-comment lines).
        # Remove line comments before counting braces.
        tmp = stripped
        sub(/\/\/.*/, "", tmp)
        n = split(tmp, chars, "")
        for (i = 1; i <= n; i++) {
          if (chars[i] == "{") test_brace_depth++
          if (chars[i] == "}") test_brace_depth--
        }
        test++
        if (test_brace_depth <= 0) in_test = 0
        next
      }

      # --- Inside a block comment (outside test): comment ---
      if (in_block_comment) {
        if (stripped ~ /\*\//) in_block_comment = 0
        comments++
        next
      }

      # --- Check for block comment opening ---
      bc = check_block_comment(stripped)
      if (bc == 1) { in_block_comment = 1; comments++; next }
      if (bc == 2) { comments++; next }
      # bc == 3: self-closing comment but also has code; fall through

      # --- Blank lines ---
      if (line ~ /^[[:space:]]*$/) { blanks++; next }

      # --- Line comments ---
      if (line ~ /^[[:space:]]*\/\//) { comments++; next }

      # --- #[cfg(test)] starts a test block ---
      if (line ~ /#\[cfg\(test\)\]/) {
        in_test = 1
        test_brace_depth = 0
        test++
        next
      }

      # --- Production code ---
      prod++
    }

    END {
      printf "%d %d %d %d %d\n", prod, test, comments, blanks, total
    }
  ' | while read -r p t c b tot; do
    echo "$nfiles $p $t $c $b $tot"
  done
}

# Width for the number column
W=11

print_crate() {
  local label=$1 files=$2 prod=$3 test=$4 comments=$5 blanks=$6 total=$7
  echo "$label"
  printf "  Files:      %s\n" "$(rfmt $W "$files")"
  printf "  Production: %s\n" "$(rfmt $W "$prod")"
  printf "  Test:       %s\n" "$(rfmt $W "$test")"
  printf "  Comments:   %s\n" "$(rfmt $W "$comments")"
  printf "  Blanks:     %s\n" "$(rfmt $W "$blanks")"
  printf "  Total:      %s\n" "$(rfmt $W "$total")"
}

grand_prod=0
grand_test=0
grand_comments=0
grand_blanks=0
grand_total=0

for crate in "${CRATES[@]}"; do
  dir="$REPO_ROOT/$crate"
  if [[ ! -d "$dir" ]]; then
    echo "Warning: $dir not found, skipping." >&2
    continue
  fi

  read -r files prod test comments blanks total < <(count_lines "$dir")

  label="$(dirname "$crate")/"
  print_crate "$label" "$files" "$prod" "$test" "$comments" "$blanks" "$total"
  echo

  grand_prod=$((grand_prod + prod))
  grand_test=$((grand_test + test))
  grand_comments=$((grand_comments + comments))
  grand_blanks=$((grand_blanks + blanks))
  grand_total=$((grand_total + total))
done

echo "Grand Total:"
printf "  Production: %s\n" "$(rfmt $W "$grand_prod")"
printf "  Test:       %s\n" "$(rfmt $W "$grand_test")"
printf "  Comments:   %s\n" "$(rfmt $W "$grand_comments")"
printf "  Blanks:     %s\n" "$(rfmt $W "$grand_blanks")"
printf "  Total:      %s\n" "$(rfmt $W "$grand_total")"
