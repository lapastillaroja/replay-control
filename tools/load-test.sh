#!/bin/bash
set -euo pipefail

# Load test script for Replay Control App using Apache Bench (ab).
# Tests multiple endpoints at increasing concurrency levels, runs a mixed
# concurrent test, and produces a summary table plus raw output file.

TARGET="${1:-http://replay.local:8080}"
DESCRIPTION="${2:-}"
REQUESTS=50
TIMESTAMP=$(date +%Y%m%d-%H%M%S)
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_DIR="${REPO_ROOT}/tools/bench-results"
mkdir -p "$RESULTS_DIR"
RAW_FILE="${RESULTS_DIR}/load-test-raw-${TIMESTAMP}.txt"

# Capture git info for traceability
GIT_HASH=$(git -C "$REPO_ROOT" rev-parse --short HEAD 2>/dev/null || echo "unknown")
GIT_SUBJECT=$(git -C "$REPO_ROOT" log -1 --format='%s' 2>/dev/null || echo "unknown")

# Endpoints to test
declare -a EP_LABELS=(
    "Homepage (heavy SSR)"
    "Search: mario (common)"
    "Search: sonic (common)"
    "Search: street fighter (multi-word)"
    "Search: a (broad)"
    "SNES games (light)"
    "Megadrive games (light)"
    "Game detail (light)"
)
declare -a EP_PATHS=(
    "/"
    "/search?q=mario"
    "/search?q=sonic"
    "/search?q=street+fighter"
    "/search?q=a"
    "/games/snes"
    "/games/megadrive"
    "/games/snes/Super%20Mario%20World%20(USA).sfc"
)

# Concurrency levels to test
CONCURRENCIES=(1 5 10 20 30)

# ─── Helpers ──────────────────────────────────────────────────────────

log() {
    echo "$@" | tee -a "$RAW_FILE"
}

log_raw() {
    echo "$@" >> "$RAW_FILE"
}

run_ab() {
    local url="$1"
    local concurrency="$2"
    local requests="$3"
    ab -n "$requests" -c "$concurrency" "$url" 2>&1
}

extract_rps() {
    grep "Requests per second" <<< "$1" | awk '{print $4}' || echo "N/A"
}

extract_mean() {
    grep "Time per request.*mean\b" <<< "$1" | head -1 | awk '{print $4}' || echo "N/A"
}

extract_p50() {
    grep "50%" <<< "$1" | awk '{print $2}' || echo "N/A"
}

extract_p95() {
    grep "95%" <<< "$1" | awk '{print $2}' || echo "N/A"
}

extract_p99() {
    grep "99%" <<< "$1" | awk '{print $2}' || echo "N/A"
}

extract_failed() {
    grep "Failed requests" <<< "$1" | awk '{print $3}' || echo "N/A"
}

# ─── Pre-flight ───────────────────────────────────────────────────────

echo "Load test: $TARGET"
echo "Raw output: $RAW_FILE"
echo ""

: > "$RAW_FILE"
log "========================================================================"
log "Load Test — $(date -Iseconds)"
log "Target: $TARGET"
log "Git: ${GIT_HASH} — ${GIT_SUBJECT}"
[[ -n "$DESCRIPTION" ]] && log "Description: $DESCRIPTION"
log "Requests per test: $REQUESTS"
log "Concurrency levels: ${CONCURRENCIES[*]}"
log "========================================================================"
log ""

# Check reachability
if ! curl -s -o /dev/null --max-time 5 "$TARGET/" 2>/dev/null; then
    echo "ERROR: Server at $TARGET is not reachable."
    exit 1
fi

# Warmup
echo "Warming up..."
for path in "${EP_PATHS[@]}"; do
    curl -s -o /dev/null --max-time 60 "${TARGET}${path}" 2>/dev/null || true
done
echo ""

# ─── Per-endpoint tests at each concurrency level ─────────────────────

# Store results for summary: results[endpoint_idx,concurrency] = "rps mean p50 p95 p99 failed"
declare -A RESULTS

for ep_idx in "${!EP_LABELS[@]}"; do
    label="${EP_LABELS[$ep_idx]}"
    path="${EP_PATHS[$ep_idx]}"
    url="${TARGET}${path}"

    echo "Testing: $label"
    log "────────────────────────────────────────────────────────────────────────"
    log "Endpoint: $label"
    log "URL: $url"
    log "────────────────────────────────────────────────────────────────────────"
    log ""

    for c in "${CONCURRENCIES[@]}"; do
        echo "  c=$c ..."
        log "--- n=$REQUESTS c=$c ---"
        log ""

        ab_out=$(run_ab "$url" "$c" "$REQUESTS")
        log_raw "$ab_out"
        log ""

        rps=$(extract_rps "$ab_out")
        mean=$(extract_mean "$ab_out")
        p50=$(extract_p50 "$ab_out")
        p95=$(extract_p95 "$ab_out")
        p99=$(extract_p99 "$ab_out")
        failed=$(extract_failed "$ab_out")

        RESULTS["${ep_idx},${c}"]="$rps $mean $p50 $p95 $p99 $failed"
    done
    echo ""
done

# ─── Mixed concurrent test ────────────────────────────────────────────

MIXED_CONCURRENCY=5
MIXED_REQUESTS=50

echo "Testing: Mixed concurrent (4 endpoints at c=$MIXED_CONCURRENCY)"
log "════════════════════════════════════════════════════════════════════════"
log "Mixed Concurrent Test"
log "4 endpoints simultaneously, each at n=$MIXED_REQUESTS c=$MIXED_CONCURRENCY"
log "════════════════════════════════════════════════════════════════════════"
log ""

# Use first 4 endpoints for mixed test
declare -a MIXED_PIDS=()
declare -a MIXED_TMPFILES=()

for i in 0 1 2 3; do
    tmpfile=$(mktemp /tmp/ab-mixed-XXXXXX.txt)
    MIXED_TMPFILES+=("$tmpfile")
    url="${TARGET}${EP_PATHS[$i]}"
    ab -n "$MIXED_REQUESTS" -c "$MIXED_CONCURRENCY" "$url" > "$tmpfile" 2>&1 &
    MIXED_PIDS+=($!)
done

# Wait for all to finish
for pid in "${MIXED_PIDS[@]}"; do
    wait "$pid" || true
done

declare -A MIXED_RESULTS

for i in 0 1 2 3; do
    label="${EP_LABELS[$i]}"
    ab_out=$(cat "${MIXED_TMPFILES[$i]}")

    log "--- Mixed: $label (n=$MIXED_REQUESTS c=$MIXED_CONCURRENCY) ---"
    log ""
    log_raw "$ab_out"
    log ""

    rps=$(extract_rps "$ab_out")
    mean=$(extract_mean "$ab_out")
    p50=$(extract_p50 "$ab_out")
    p95=$(extract_p95 "$ab_out")
    p99=$(extract_p99 "$ab_out")
    failed=$(extract_failed "$ab_out")

    MIXED_RESULTS["$i"]="$rps $mean $p50 $p95 $p99 $failed"

    rm -f "${MIXED_TMPFILES[$i]}"
done

echo ""

# ─── Summary table ────────────────────────────────────────────────────

print_summary() {
    local dest="$1"  # "both" or "raw"

    out() {
        if [[ "$dest" == "both" ]]; then
            echo "$@" | tee -a "$RAW_FILE"
        else
            echo "$@" >> "$RAW_FILE"
        fi
    }

    out ""
    out "========================================================================"
    out "SUMMARY"
    out "========================================================================"
    out ""

    for ep_idx in "${!EP_LABELS[@]}"; do
        label="${EP_LABELS[$ep_idx]}"
        out "$label"
        out "$(printf "  %-5s  %10s  %10s  %10s  %10s  %10s  %7s" "Conc" "Req/s" "Mean(ms)" "P50(ms)" "P95(ms)" "P99(ms)" "Failed")"
        out "$(printf "  %-5s  %10s  %10s  %10s  %10s  %10s  %7s" "----" "-----" "--------" "-------" "-------" "-------" "------")"

        for c in "${CONCURRENCIES[@]}"; do
            data="${RESULTS["${ep_idx},${c}"]:-N/A N/A N/A N/A N/A N/A}"
            read -r rps mean p50 p95 p99 failed <<< "$data"
            out "$(printf "  %-5s  %10s  %10s  %10s  %10s  %10s  %7s" "$c" "$rps" "$mean" "$p50" "$p95" "$p99" "$failed")"
        done
        out ""
    done

    out "Mixed Concurrent Test (4 endpoints simultaneously, c=$MIXED_CONCURRENCY)"
    out "$(printf "  %-30s  %10s  %10s  %10s  %10s  %10s  %7s" "Endpoint" "Req/s" "Mean(ms)" "P50(ms)" "P95(ms)" "P99(ms)" "Failed")"
    out "$(printf "  %-30s  %10s  %10s  %10s  %10s  %10s  %7s" "--------" "-----" "--------" "-------" "-------" "-------" "------")"

    for i in 0 1 2 3; do
        label="${EP_LABELS[$i]}"
        data="${MIXED_RESULTS["$i"]:-N/A N/A N/A N/A N/A N/A}"
        read -r rps mean p50 p95 p99 failed <<< "$data"
        out "$(printf "  %-30s  %10s  %10s  %10s  %10s  %10s  %7s" "$label" "$rps" "$mean" "$p50" "$p95" "$p99" "$failed")"
    done
    out ""
}

print_summary "both"

log "========================================================================"
log "End of load test — $(date -Iseconds)"
log "========================================================================"

echo "Raw output saved to: $RAW_FILE"
echo "Done."
