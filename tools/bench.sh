#!/bin/bash
set -euo pipefail

# Performance benchmark suite for Replay Control App.
# Measures TTFB, total time, response sizes, asset sizes,
# optional Lighthouse scores, and optional load testing.

TARGET="http://localhost:8091"
TAG=""
SKIP_LIGHTHOUSE=false
SKIP_AB=false
RUNS=3
RESULTS_DIR="$(cd "$(dirname "$0")" && pwd)/bench-results"

usage() {
    cat <<EOF
Usage: $(basename "$0") [OPTIONS]

Options:
  --target <url>      Server URL (default: http://localhost:8091)
  --tag <name>        Tag for this benchmark run (e.g., "baseline", "after-tier1")
  --skip-lighthouse   Skip Lighthouse headless tests
  --skip-ab           Skip Apache Bench load tests
  --runs <n>          Number of runs per endpoint (default: 3)
  -h, --help          Show this help
EOF
    exit 0
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --target)    TARGET="$2"; shift 2 ;;
        --tag)       TAG="$2"; shift 2 ;;
        --skip-lighthouse) SKIP_LIGHTHOUSE=true; shift ;;
        --skip-ab)   SKIP_AB=true; shift ;;
        --runs)      RUNS="$2"; shift 2 ;;
        -h|--help)   usage ;;
        *) echo "Unknown option: $1"; usage ;;
    esac
done

# Ensure results directory exists.
mkdir -p "$RESULTS_DIR"

TIMESTAMP=$(date +%Y%m%d-%H%M%S)
if [[ -n "$TAG" ]]; then
    JSON_FILE="$RESULTS_DIR/${TIMESTAMP}-${TAG}.json"
else
    JSON_FILE="$RESULTS_DIR/${TIMESTAMP}.json"
fi

# ─── Helpers ───────────────────────────────────────────────────────

median() {
    sort -n | awk '{a[NR]=$1} END {
        if (NR%2==1) print a[(NR+1)/2];
        else printf "%.3f\n", (a[NR/2]+a[NR/2+1])/2
    }'
}

# Measure a single curl request. Outputs: ttfb_ms total_ms size_kb
curl_measure() {
    local url="$1"
    local method="${2:-GET}"
    local data="${3:-}"

    local args=(
        -s -o /dev/null
        -w '%{time_starttransfer} %{time_total} %{size_download}'
        --compressed
        --max-time 180
    )

    if [[ "$method" == "POST" ]]; then
        args+=(-X POST -H "Content-Type: application/x-www-form-urlencoded")
        if [[ -n "$data" ]]; then
            args+=(--data-raw "$data")
        fi
    fi

    local result
    result=$(curl "${args[@]}" "$url" 2>/dev/null || echo "0 0 0")
    local ttfb total size
    read -r ttfb total size <<< "$result"

    local ttfb_ms total_ms size_kb
    ttfb_ms=$(echo "$ttfb * 1000" | bc -l)
    total_ms=$(echo "$total * 1000" | bc -l)
    size_kb=$(echo "scale=1; $size / 1024" | bc -l)

    echo "$ttfb_ms $total_ms $size_kb"
}

# Run N measurements, compute medians.
bench_endpoint() {
    local label="$1"
    local url="$2"
    local method="${3:-GET}"
    local data="${4:-}"

    local ttfbs=() totals=() sizes=()

    for ((i=1; i<=RUNS; i++)); do
        local result
        result=$(curl_measure "$url" "$method" "$data")
        local t1 t2 s
        read -r t1 t2 s <<< "$result"
        ttfbs+=("$t1")
        totals+=("$t2")
        sizes+=("$s")
    done

    local med_ttfb med_total med_size
    med_ttfb=$(printf '%s\n' "${ttfbs[@]}" | median)
    med_total=$(printf '%s\n' "${totals[@]}" | median)
    med_size=$(printf '%s\n' "${sizes[@]}" | median)

    printf "  %-30s  %8.1f ms  %8.1f ms  %8.1f KB\n" "$label" "$med_ttfb" "$med_total" "$med_size"

    ENDPOINT_RESULTS+=("$(printf '{"name":"%s","ttfb_ms":%.1f,"total_ms":%.1f,"size_kb":%.1f}' \
        "$label" "$med_ttfb" "$med_total" "$med_size")")
}

# ─── Check server is reachable ─────────────────────────────────────

echo "Benchmarking: $TARGET"
echo "Runs per endpoint: $RUNS"
[[ -n "$TAG" ]] && echo "Tag: $TAG"
echo ""

if ! curl -s -o /dev/null --max-time 5 "$TARGET/manifest.json" 2>/dev/null; then
    echo "ERROR: Server at $TARGET is not reachable."
    exit 1
fi

# ─── Warmup pass ───────────────────────────────────────────────────
# Hit each page once so caches are warm; we measure steady-state.

echo "Warming up caches..."
curl -s -o /dev/null --max-time 180 "$TARGET/" 2>/dev/null || true
curl -s -o /dev/null --max-time 180 "$TARGET/games/nintendo_nes" 2>/dev/null || true
curl -s -o /dev/null --max-time 180 "$TARGET/games/arcade_mame" 2>/dev/null || true
echo ""

# ─── Part 1: Server-side endpoint benchmarks ──────────────────────

declare -a ENDPOINT_RESULTS=()

echo "=== Server-side Endpoints (median of $RUNS runs, warm cache) ==="
echo ""
printf "  %-30s  %11s  %11s  %11s\n" "Endpoint" "TTFB" "Total" "Size"
printf "  %-30s  %11s  %11s  %11s\n" "--------" "----" "-----" "----"

bench_endpoint "Home /" \
    "${TARGET}/"

bench_endpoint "Games NES" \
    "${TARGET}/games/nintendo_nes"

bench_endpoint "Games Arcade" \
    "${TARGET}/games/arcade_mame"

# NOTE: Direct server fn calls via curl don't work reliably because Leptos 0.7
# uses a custom codec. The SSR page benchmarks above are the real user-facing
# metrics since server fns run inline during SSR.

echo ""

# ─── Part 2: Asset sizes ──────────────────────────────────────────

echo "=== Asset Sizes ==="
echo ""

SITE_ROOT="$(cd "$(dirname "$0")/.." && pwd)/target/site"
declare -a ASSET_RESULTS=()

measure_asset() {
    local label="$1"
    local path="$2"
    if [[ -f "$path" ]]; then
        local raw_kb gz_kb
        raw_kb=$(echo "scale=1; $(stat -c%s "$path") / 1024" | bc -l)
        gz_kb=$(echo "scale=1; $(gzip -c "$path" | wc -c) / 1024" | bc -l)
        printf "  %-30s  %8.1f KB raw  %8.1f KB gzip\n" "$label" "$raw_kb" "$gz_kb"
        ASSET_RESULTS+=("$(printf '{"name":"%s","raw_kb":%.1f,"gzip_kb":%.1f}' "$label" "$raw_kb" "$gz_kb")")
    else
        printf "  %-30s  (not found: %s)\n" "$label" "$path"
        ASSET_RESULTS+=("$(printf '{"name":"%s","raw_kb":0,"gzip_kb":0}' "$label")")
    fi
}

# Discover the hashed wasm URL from the home HTML so bench.sh tracks whatever
# the server is actually serving (Leptos emits the hashed asset URL in the
# <link rel="preload"> tag).
HOME_HTML=$(curl -s --max-time 60 "$TARGET/" 2>/dev/null || true)
WASM_URL_PATH=$(echo "$HOME_HTML" | grep -oE 'href="[^"]*replay_control_app\.[a-f0-9]+\.wasm"' | head -1 | sed 's/^href="//; s/"$//' || true)
if [[ -z "$WASM_URL_PATH" ]]; then
    echo "WARN: could not discover hashed wasm URL from $TARGET/ — falling back to legacy filename" >&2
    WASM_URL_PATH="/static/pkg/replay_control_app_bg.wasm"
fi
WASM_FILENAME=$(basename "$WASM_URL_PATH")
WASM_FILE="$SITE_ROOT/pkg/$WASM_FILENAME"
CSS_FILE="$SITE_ROOT/style.css"

measure_asset "WASM bundle" "$WASM_FILE"
measure_asset "CSS" "$CSS_FILE"

# HTML sizes via curl (uncompressed to measure actual content size).
home_size=$(echo -n "$HOME_HTML" | wc -c)
games_size=$(curl -s --max-time 60 "${TARGET}/games/nintendo_nes" 2>/dev/null | wc -c)
home_kb=$(echo "scale=1; $home_size / 1024" | bc -l)
games_kb=$(echo "scale=1; $games_size / 1024" | bc -l)
printf "  %-30s  %8.1f KB\n" "Home HTML" "$home_kb"
printf "  %-30s  %8.1f KB\n" "Games NES HTML" "$games_kb"
ASSET_RESULTS+=("$(printf '{"name":"Home HTML","raw_kb":%.1f,"gzip_kb":0}' "$home_kb")")
ASSET_RESULTS+=("$(printf '{"name":"Games NES HTML","raw_kb":%.1f,"gzip_kb":0}' "$games_kb")")

# Check gzip on WASM response.
wasm_encoding=$(curl -sI "${TARGET}${WASM_URL_PATH}" -H "Accept-Encoding: gzip" 2>/dev/null | grep -i "content-encoding" | tr -d '\r' || echo "")
if [[ -n "$wasm_encoding" ]]; then
    printf "  %-30s  %s\n" "WASM Content-Encoding" "$wasm_encoding"
else
    printf "  %-30s  (none — not compressed)\n" "WASM Content-Encoding"
fi

echo ""

# ─── Part 3: Lighthouse (optional) ────────────────────────────────

declare -a LIGHTHOUSE_RESULTS=()

if [[ "$SKIP_LIGHTHOUSE" == "false" ]] && command -v lighthouse &>/dev/null; then
    echo "=== Lighthouse (headless Chrome) ==="
    echo ""

    run_lighthouse() {
        local label="$1"
        local url="$2"
        local tmp_json
        tmp_json=$(mktemp /tmp/lh-XXXXXX.json)

        lighthouse "$url" \
            --output=json \
            --output-path="$tmp_json" \
            --chrome-flags="--headless --no-sandbox" \
            --only-categories=performance \
            --quiet 2>/dev/null || true

        if [[ -f "$tmp_json" ]] && python3 -c "import json; json.load(open('$tmp_json'))" 2>/dev/null; then
            local perf lcp cls tbt si
            perf=$(python3 -c "import json; d=json.load(open('$tmp_json')); print(d['categories']['performance']['score']*100)" 2>/dev/null || echo "N/A")
            lcp=$(python3 -c "import json; d=json.load(open('$tmp_json')); print(d['audits']['largest-contentful-paint']['numericValue'])" 2>/dev/null || echo "N/A")
            cls=$(python3 -c "import json; d=json.load(open('$tmp_json')); print(d['audits']['cumulative-layout-shift']['numericValue'])" 2>/dev/null || echo "N/A")
            tbt=$(python3 -c "import json; d=json.load(open('$tmp_json')); print(d['audits']['total-blocking-time']['numericValue'])" 2>/dev/null || echo "N/A")
            si=$(python3 -c "import json; d=json.load(open('$tmp_json')); print(d['audits']['speed-index']['numericValue'])" 2>/dev/null || echo "N/A")

            printf "  %-25s  Perf: %s  LCP: %s ms  CLS: %s  TBT: %s ms  SI: %s ms\n" \
                "$label" "$perf" "$lcp" "$cls" "$tbt" "$si"
            LIGHTHOUSE_RESULTS+=("$(printf '{"name":"%s","performance":%s,"lcp_ms":%s,"cls":%s,"tbt_ms":%s,"speed_index_ms":%s}' \
                "$label" "${perf:-0}" "${lcp:-0}" "${cls:-0}" "${tbt:-0}" "${si:-0}")")
        else
            printf "  %-25s  (failed)\n" "$label"
        fi

        rm -f "$tmp_json"
    }

    run_lighthouse "Home /" "${TARGET}/"
    run_lighthouse "Games NES" "${TARGET}/games/nintendo_nes"
    echo ""
elif [[ "$SKIP_LIGHTHOUSE" == "false" ]]; then
    echo "(Skipping Lighthouse — not installed)"
    echo ""
fi

# ─── Part 4: Load testing with ab (optional) ──────────────────────

declare -a LOADTEST_RESULTS=()

if [[ "$SKIP_AB" == "false" ]] && command -v ab &>/dev/null; then
    echo "=== Load Test (ab: 50 requests, 10 concurrent) ==="
    echo ""

    run_ab() {
        local label="$1"
        local url="$2"
        local ab_out
        ab_out=$(ab -n 50 -c 10 "$url" 2>/dev/null || true)

        local rps mean_ms p95_ms
        rps=$(echo "$ab_out" | grep "Requests per second" | awk '{print $4}')
        mean_ms=$(echo "$ab_out" | grep "Time per request.*mean\b" | head -1 | awk '{print $4}')
        p95_ms=$(echo "$ab_out" | grep "95%" | awk '{print $2}')

        if [[ -n "$rps" ]]; then
            printf "  %-25s  %8s req/s  mean: %s ms  P95: %s ms\n" "$label" "$rps" "$mean_ms" "$p95_ms"
            LOADTEST_RESULTS+=("$(printf '{"name":"%s","rps":%s,"mean_ms":%s,"p95_ms":%s}' \
                "$label" "${rps:-0}" "${mean_ms:-0}" "${p95_ms:-0}")")
        else
            printf "  %-25s  (failed)\n" "$label"
        fi
    }

    run_ab "Home /" "${TARGET}/"
    run_ab "Games NES" "${TARGET}/games/nintendo_nes"
    echo ""
elif [[ "$SKIP_AB" == "false" ]]; then
    echo "(Skipping load test — ab not installed)"
    echo ""
fi

# ─── Write JSON results ───────────────────────────────────────────

{
    echo "{"
    echo "  \"timestamp\": \"$(date -Iseconds)\","
    echo "  \"target\": \"$TARGET\","
    [[ -n "$TAG" ]] && echo "  \"tag\": \"$TAG\","
    echo "  \"runs\": $RUNS,"

    echo "  \"endpoints\": ["
    for i in "${!ENDPOINT_RESULTS[@]}"; do
        [[ $i -lt $((${#ENDPOINT_RESULTS[@]}-1)) ]] && sep="," || sep=""
        echo "    ${ENDPOINT_RESULTS[$i]}${sep}"
    done
    echo "  ],"

    echo "  \"assets\": ["
    for i in "${!ASSET_RESULTS[@]}"; do
        [[ $i -lt $((${#ASSET_RESULTS[@]}-1)) ]] && sep="," || sep=""
        echo "    ${ASSET_RESULTS[$i]}${sep}"
    done
    echo "  ],"

    echo "  \"lighthouse\": ["
    if [[ ${#LIGHTHOUSE_RESULTS[@]} -gt 0 ]]; then
        for i in "${!LIGHTHOUSE_RESULTS[@]}"; do
            [[ $i -lt $((${#LIGHTHOUSE_RESULTS[@]}-1)) ]] && sep="," || sep=""
            echo "    ${LIGHTHOUSE_RESULTS[$i]}${sep}"
        done
    fi
    echo "  ],"

    echo "  \"loadtest\": ["
    if [[ ${#LOADTEST_RESULTS[@]} -gt 0 ]]; then
        for i in "${!LOADTEST_RESULTS[@]}"; do
            [[ $i -lt $((${#LOADTEST_RESULTS[@]}-1)) ]] && sep="," || sep=""
            echo "    ${LOADTEST_RESULTS[$i]}${sep}"
        done
    fi
    echo "  ]"

    echo "}"
} > "$JSON_FILE"

echo "Results saved to: $JSON_FILE"
echo "Done."
