#!/bin/bash
# tools/user-load-test.sh — persona-based parallel user-load harness (oha).
#
# Simulates a handful of REAL USERS hitting the deployed app concurrently — one
# oha process per persona — and reports per-persona throughput/latency plus
# Pi-side CPU/RSS sampled during the run. Doubles as a regression gate: full
# runs can be saved as named baselines and later runs compared against them
# with thresholded verdicts (non-zero exit on regression).
#
# THE PERSONA MODEL — one concurrently-running oha process each:
#
#   browser           GET page shells: home, /games/<system>, real game-detail
#                     pages, /search?q= pages. URLs picked randomly per request
#                     (--urls-from-file). This is the SSR/navigation cost only:
#                     the game grid itself is NOT in the SSR — a real browser
#                     hydrates and then POSTs to /sfn/ for the data. Hence:
#   scroller_shallow  POST get_roms_page offset=0    — the server fn that
#   scroller_deep     POST get_roms_page offset=N/2  — actually renders the
#                     grid; deep vs shallow exposes OFFSET-pagination cost.
#   searcher_common   POST global_search query=mario — a typical search.
#   searcher_broad    POST global_search query=a     — broad worst-case search
#                     at a deliberately low rate.
#   detail_reader     POST get_related_games for a real game — the game-detail
#                     data path.
#
#   Each persona is rate-limited (-q) so the aggregate (~8.5 req/s) models a
#   handful of active humans, not a DoS. --latency-correction avoids
#   coordinated omission under rate limiting. oha's fixed-body-per-process
#   limitation is why searcher/scroller are split into sub-personas instead of
#   one process rotating bodies.
#
# WORKS AGAINST ANY DEPLOYED RELEASE (>= v1.0.0):
#   /sfn/<name><hash> URL hashes change per build, so they are discovered at
#   runtime from the DEPLOYED wasm (hashed URL scraped from the public /login
#   page, wasm downloaded, `strings | grep`). Personas whose server fn doesn't
#   exist on the target build are skipped and noted, not failed. Readiness
#   uses /api/core/status when present, else falls back to an authed GET / +
#   settle wait. /api/version (public on all versions) records the app version.
#
# USAGE
#   tools/user-load-test.sh                          # full run (150s), keep-alive
#   tools/user-load-test.sh --smoke                  # 15s shakeout run
#   tools/user-load-test.sh --duration 300           # custom measurement window
#   tools/user-load-test.sh --cold                   # --disable-keepalive: adds
#                                                    # per-request TLS handshake
#                                                    # cost (comparison mode)
#   tools/user-load-test.sh --save-baseline main     # record run as baseline
#   tools/user-load-test.sh --compare main           # diff vs baseline + verdict
#   tools/user-load-test.sh --no-compare             # suppress auto-compare
#   tools/user-load-test.sh --ip 192.168.1.50        # override target host
#
#   When a baseline named "main" exists and no --compare/--no-compare is given,
#   comparison against "main" runs automatically (post-deploy gate: deploy,
#   run, non-zero exit means investigate).
#
# REGRESSION THRESHOLDS (env-overridable)
#   This Pi shows +/-5-7% run-to-run noise on identical builds (measured), so
#   thresholds sit comfortably above that. Below-threshold drift is reported
#   as "within noise" — don't chase phantom regressions under these values.
#     UL_TH_RPS_DROP  rps drop %            (default 15)
#     UL_TH_LAT_UP    p50/p99 increase %    (default 25)
#     UL_TH_RSS_UP    load RSS increase %   (default 20)
#   Any NEW non-2xx / transport errors vs baseline are always a regression.
#
# ENVIRONMENT
#   PI_IP (replay.local)  PI_PORT (8443)  PI_SCHEME (https)  PI_PASS (replayos)
#   UL_ADMIN_PASSWORD     app admin password (default: PI_PASS)
#   UL_READY_TIMEOUT      seconds to wait for readiness (default 180)
#
# OUTPUT
#   tools/bench-results/user-load-<timestamp>/  raw oha JSON per persona,
#       device samples, summary.json (machine-readable), summary.md (report)
#   tools/bench-results/user-load-baselines/<label>.json  saved baselines
#
# Requires: oha (cargo install oha), jq, curl. Uses tools/pi-cpu.sh and
# tools/pi-memory.sh over SSH for device-side sampling (failures tolerated).

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"

# ---------------------------------------------------------------------------
# Configuration and flags
# ---------------------------------------------------------------------------
PI_IP="${PI_IP:-replay.local}"
PI_PORT="${PI_PORT:-8443}"
PI_SCHEME="${PI_SCHEME:-https}"
PI_PASS="${PI_PASS:-replayos}"
ADMIN_PASSWORD="${UL_ADMIN_PASSWORD:-$PI_PASS}"
READY_TIMEOUT="${UL_READY_TIMEOUT:-180}"

TH_RPS_DROP="${UL_TH_RPS_DROP:-15}"
TH_LAT_UP="${UL_TH_LAT_UP:-25}"
TH_RSS_UP="${UL_TH_RSS_UP:-20}"

DURATION=150
COLD=false
SAVE_BASELINE=""
COMPARE=""
NO_COMPARE=false

while [[ $# -gt 0 ]]; do
    case "$1" in
        --duration)      DURATION="$2"; shift 2 ;;
        --smoke)         DURATION=15; shift ;;
        --cold)          COLD=true; shift ;;
        --save-baseline) SAVE_BASELINE="$2"; shift 2 ;;
        --compare)       COMPARE="$2"; shift 2 ;;
        --no-compare)    NO_COMPARE=true; shift ;;
        --ip)            PI_IP="$2"; shift 2 ;;
        -h|--help)       sed -n '2,/^$/p' "$0" | sed 's/^# \?//'; exit 0 ;;
        *)               echo "Unknown option: $1" >&2; exit 2 ;;
    esac
done

BASE="${PI_SCHEME}://${PI_IP}:${PI_PORT}"
MODE=$([[ "$COLD" == true ]] && echo "cold" || echo "keepalive")
TS="$(date -u +%Y%m%d-%H%M%S)"
RESULTS_ROOT="$SCRIPT_DIR/bench-results"
RUN_DIR="$RESULTS_ROOT/user-load-$TS"
BASELINE_DIR="$RESULTS_ROOT/user-load-baselines"
mkdir -p "$RUN_DIR/personas" "$RUN_DIR/device" "$BASELINE_DIR"

command -v oha >/dev/null || { echo "ERROR: oha not found (cargo install oha)" >&2; exit 2; }
command -v jq  >/dev/null || { echo "ERROR: jq not found" >&2; exit 2; }

log()  { echo "[$(date -u +%H:%M:%S)] $*"; }
die()  { echo "ERROR: $*" >&2; exit 2; }
uri()  { jq -rn --arg v "$1" '$v|@uri'; }

PERSONA_PIDS=()
cleanup() {
    for pid in ${PERSONA_PIDS[@]+"${PERSONA_PIDS[@]}"}; do kill "$pid" 2>/dev/null; done
}
trap cleanup EXIT

# ---------------------------------------------------------------------------
# Step 1: target version + readiness
# ---------------------------------------------------------------------------
log "Target: $BASE (mode: $MODE, duration: ${DURATION}s)"

VERSION_JSON=$(curl -sk --max-time 10 "$BASE/api/version" || true)
APP_VERSION=$(jq -r '.version // "unknown"' <<<"$VERSION_JSON" 2>/dev/null || echo unknown)
APP_GIT_HASH=$(jq -r '.git_hash // "unknown"' <<<"$VERSION_JSON" 2>/dev/null || echo unknown)
[[ "$APP_VERSION" == "unknown" ]] && die "cannot reach $BASE/api/version — is the app up?"
log "Deployed app: v$APP_VERSION ($APP_GIT_HASH)"

# Readiness: prefer /api/core/status (recent builds); fall back to reachability
# + fixed settle wait on older builds that lack the endpoint.
READINESS_NOTE=""
STATUS_JSON=$(curl -sk --max-time 10 "$BASE/api/core/status" || true)
if jq -e '.ready != null' <<<"$STATUS_JSON" >/dev/null 2>&1; then
    deadline=$(( SECONDS + READY_TIMEOUT ))
    until jq -e '.ready == true and .activity == "idle"' <<<"$STATUS_JSON" >/dev/null 2>&1; do
        (( SECONDS >= deadline )) && die "app not ready+idle within ${READY_TIMEOUT}s"
        log "waiting for ready+idle... ($(jq -r '.activity // "?"' <<<"$STATUS_JSON"))"
        sleep 5
        STATUS_JSON=$(curl -sk --max-time 10 "$BASE/api/core/status" || true)
    done
    TOTAL_ROMS=$(jq -r '.total_roms // 0' <<<"$STATUS_JSON")
    log "App ready+idle, $TOTAL_ROMS games"
else
    READINESS_NOTE="no /api/core/status on this build: used reachability + 30s settle"
    log "WARN: $READINESS_NOTE"
    STATUS_JSON=""
    TOTAL_ROMS=0
    sleep 30
fi

# ---------------------------------------------------------------------------
# Step 2: discover server-fn hashes from the DEPLOYED wasm
# ---------------------------------------------------------------------------
# /login and /static/* are public on all versions. Leptos emits the hashed
# wasm URL in the page head; the /sfn/<name><hash> strings live in the binary.
log "Discovering server-fn hashes from the deployed wasm..."
LOGIN_HTML=$(curl -sk --max-time 30 "$BASE/login" || true)
WASM_PATH=$(grep -oE 'href="[^"]*replay_control_app\.[a-f0-9]+\.wasm"' <<<"$LOGIN_HTML" \
            | head -1 | sed 's/^href="//; s/"$//')
[[ -z "$WASM_PATH" ]] && WASM_PATH="/static/pkg/replay_control_app_bg.wasm"  # pre-hashed-URL builds

WASM_FILE="$RUN_DIR/deployed.wasm"
curl -sk --max-time 120 -H 'Accept-Encoding: identity' -o "$WASM_FILE" "$BASE$WASM_PATH" \
    || die "failed to download deployed wasm from $BASE$WASM_PATH"
# Tolerate a server that compressed anyway (wasm magic is \0asm).
if [[ "$(head -c2 "$WASM_FILE" | od -An -tx1 | tr -d ' ')" == "1f8b" ]]; then
    mv "$WASM_FILE" "$WASM_FILE.gz" && gunzip "$WASM_FILE.gz"
fi

HASHES_FILE="$RUN_DIR/sfn-hashes.txt"
strings "$WASM_FILE" \
    | grep -oE '(login_admin|get_roms_page|global_search|get_related_games|get_info)[0-9]{5,}' \
    | sort -u > "$HASHES_FILE"
rm -f "$WASM_FILE"  # 6+ MB; hashes are all we need

# Echoes the full /sfn/ URL for a server fn, or nothing if this build lacks it.
sfn_url() {
    local hash
    hash=$(grep -E "^$1[0-9]+$" "$HASHES_FILE" | head -1)
    [[ -n "$hash" ]] && echo "$BASE/sfn/$hash"
}
log "Found $(wc -l < "$HASHES_FILE") server-fn hashes"

# ---------------------------------------------------------------------------
# Step 3: login (once) and reuse the session cookie
# ---------------------------------------------------------------------------
LOGIN_URL=$(sfn_url login_admin)
[[ -z "$LOGIN_URL" ]] && die "login_admin not found in deployed wasm"
COOKIE=$(curl -sk -X POST "$LOGIN_URL" \
    -H 'Content-Type: application/x-www-form-urlencoded' \
    -H 'sec-fetch-site: same-origin' \
    --data "password=$(uri "$ADMIN_PASSWORD")" -D - -o /dev/null \
    | grep -oiE 'ReplayControlSession=[^;]+' | head -1)
[[ -z "$COOKIE" ]] && die "login failed — no session cookie returned"
log "Logged in, session cookie acquired"

# POST a server fn with the same-origin signal the CSRF gate requires.
sfn_post() { # url body
    curl -sk --max-time 60 -X POST "$1" \
        -H "Cookie: $COOKIE" -H 'sec-fetch-site: same-origin' \
        -H 'Content-Type: application/x-www-form-urlencoded' --data "$2"
}

# ---------------------------------------------------------------------------
# Step 4: build the workload from REAL library data
# ---------------------------------------------------------------------------
ROMS_URL=$(sfn_url get_roms_page)
SEARCH_URL=$(sfn_url global_search)
RELATED_URL=$(sfn_url get_related_games)
INFO_URL=$(sfn_url get_info)

# Storage kind for baseline metadata (best-effort).
STORAGE_KIND="unknown"
if [[ -n "$INFO_URL" ]]; then
    STORAGE_KIND=$(sfn_post "$INFO_URL" "" | jq -r '.storage_kind // "unknown"' 2>/dev/null || echo unknown)
fi

# Pick up to 4 populated systems: from /api/core/status when available,
# else probe a candidate list via get_roms_page.
SYSTEMS=()
if [[ -n "$STATUS_JSON" ]]; then
    while IFS= read -r s; do SYSTEMS+=("$s"); done < <(
        jq -r '.systems | to_entries | sort_by(-.value.roms) | .[:4][] | .key' <<<"$STATUS_JSON")
elif [[ -n "$ROMS_URL" ]]; then
    for cand in nintendo_snes snes sega_smd megadrive nintendo_nes nes arcade_fbneo arcade_mame; do
        total=$(sfn_post "$ROMS_URL" "system=$cand&offset=0&limit=1&search=" \
                | jq -r '.total // 0' 2>/dev/null || echo 0)
        [[ "$total" -gt 0 ]] && SYSTEMS+=("$cand")
        [[ ${#SYSTEMS[@]} -ge 4 ]] && break
    done
fi
[[ ${#SYSTEMS[@]} -eq 0 ]] && die "could not find any populated system"
GRID_SYSTEM="${SYSTEMS[0]}"
log "Systems: ${SYSTEMS[*]} (grid persona uses: $GRID_SYSTEM)"

# Sample real ROMs from the grid system: filenames for detail URLs + the total
# for the deep-scroll offset. Works on all versions (RomPage has .total).
SAMPLE_JSON=$(sfn_post "$ROMS_URL" "system=$GRID_SYSTEM&offset=0&limit=50&search=")
SYSTEM_TOTAL=$(jq -r '.total // 0' <<<"$SAMPLE_JSON")
[[ "$SYSTEM_TOTAL" -gt 0 ]] || die "get_roms_page returned no games for $GRID_SYSTEM"
[[ "$TOTAL_ROMS" -eq 0 ]] && TOTAL_ROMS="$SYSTEM_TOTAL"
DEEP_OFFSET=$(( SYSTEM_TOTAL / 2 / 100 * 100 ))

# Detail-reader target: prefer a known series-rich game for a stable, deep
# related-games query; fall back to the first sampled ROM.
DETAIL_GAME=$(sfn_post "$ROMS_URL" "system=$GRID_SYSTEM&offset=0&limit=1&search=$(uri 'Super Mario World')" \
              | jq -r '.roms[0].rom_filename // empty')
[[ -z "$DETAIL_GAME" ]] && DETAIL_GAME=$(jq -r '.roms[0].rom_filename' <<<"$SAMPLE_JSON")
log "Detail game: $GRID_SYSTEM / $DETAIL_GAME (deep offset: $DEEP_OFFSET of $SYSTEM_TOTAL)"

# Browser persona URL pool: home + system pages + search pages + real detail
# pages (every 6th sampled filename). oha picks a random line per request.
URLS_FILE="$RUN_DIR/browser-urls.txt"
{
    echo "$BASE/"
    for s in "${SYSTEMS[@]}"; do echo "$BASE/games/$s"; done
    echo "$BASE/search?q=mario"
    echo "$BASE/search?q=sonic"
    echo "$BASE/search?q=street%20fighter"
    jq -r '.roms[].rom_filename' <<<"$SAMPLE_JSON" | awk 'NR % 6 == 1' | head -8 \
        | while IFS= read -r f; do echo "$BASE/games/$GRID_SYSTEM/$(uri "$f")"; done
} > "$URLS_FILE"
log "Browser URL pool: $(wc -l < "$URLS_FILE") page URLs"

# ---------------------------------------------------------------------------
# Step 5: persona definitions
# ---------------------------------------------------------------------------
# name | connections | rate (req/s) | kind (get|post) | target | body
# Aggregate steady rate ~8.5 req/s == a handful of humans actively clicking.
SEARCH_COMMON_BODY="query=mario&hide_hacks=false&hide_translations=false&hide_betas=false&hide_clones=false&genre=&per_system_limit=10"
SEARCH_BROAD_BODY="query=a&hide_hacks=false&hide_translations=false&hide_betas=false&hide_clones=false&genre=&per_system_limit=10"
PERSONAS=(
    "browser|3|3|get|$URLS_FILE|"
    "scroller_shallow|2|2|post|$ROMS_URL|system=$GRID_SYSTEM&offset=0&limit=100&search="
    "scroller_deep|1|1|post|$ROMS_URL|system=$GRID_SYSTEM&offset=$DEEP_OFFSET&limit=100&search="
    "searcher_common|1|1|post|$SEARCH_URL|$SEARCH_COMMON_BODY"
    "searcher_broad|1|0.5|post|$SEARCH_URL|$SEARCH_BROAD_BODY"
    "detail_reader|1|1|post|$RELATED_URL|system=$GRID_SYSTEM&filename=$(uri "$DETAIL_GAME")"
)

COLD_ARGS=()
[[ "$COLD" == true ]] && COLD_ARGS=(--disable-keepalive)

# ---------------------------------------------------------------------------
# Step 6: idle device baseline + warm-up
# ---------------------------------------------------------------------------
pi_mem() { PI_IP="$PI_IP" PI_PASS="$PI_PASS" "$SCRIPT_DIR/pi-memory.sh" --json 2>/dev/null; }
pi_cpu() { # duration
    PI_IP="$PI_IP" PI_PASS="$PI_PASS" PI_SCHEME="$PI_SCHEME" PI_PORT="$PI_PORT" \
        PI_COOKIE="$COOKIE" "$SCRIPT_DIR/pi-cpu.sh" --json --duration "$1" 2>/dev/null;
}

log "Sampling idle device baseline..."
pi_mem > "$RUN_DIR/device/idle-mem.json" || log "WARN: idle memory sample failed"
pi_cpu 8 > "$RUN_DIR/device/idle-cpu.json" || log "WARN: idle CPU sample failed"

# Warm-up: touch every persona target once or twice so TLS session caches, the
# response cache, and OS page cache are warm before measurement starts.
log "Warm-up pass..."
head -6 "$URLS_FILE" | while IFS= read -r u; do
    curl -sk --max-time 60 -H "Cookie: $COOKIE" -o /dev/null "$u"
done
for spec in "${PERSONAS[@]}"; do
    IFS='|' read -r _name _c _q kind target body <<<"$spec"
    [[ "$kind" == post && -n "$target" ]] && sfn_post "$target" "$body" >/dev/null
done
log "Warm-up done"

# ---------------------------------------------------------------------------
# Step 7: run all personas CONCURRENTLY, sampling the device mid-run
# ---------------------------------------------------------------------------
log "Starting ${#PERSONAS[@]} personas for ${DURATION}s..."
STARTED=() SKIPPED=()
for spec in "${PERSONAS[@]}"; do
    IFS='|' read -r name conc qps kind target body <<<"$spec"
    if [[ "$kind" == post && -z "$target" ]]; then
        SKIPPED+=("$name")
        log "  skip $name: server fn not present on this build"
        continue
    fi
    args=(--insecure --no-tui -z "${DURATION}s" -c "$conc" -q "$qps" --latency-correction
          --output-format json -o "$RUN_DIR/personas/$name.json"
          -H "Cookie: $COOKIE" ${COLD_ARGS[@]+"${COLD_ARGS[@]}"})
    if [[ "$kind" == get ]]; then
        args+=(--urls-from-file "$target")
    else
        args+=(-m POST -T 'application/x-www-form-urlencoded'
               -H 'sec-fetch-site: same-origin' -d "$body" "$target")
    fi
    oha "${args[@]}" >/dev/null 2>"$RUN_DIR/personas/$name.err" &
    PERSONA_PIDS+=($!)
    STARTED+=("$name")
    log "  $name: c=$conc q=$qps/s ($kind)"
done
[[ ${#STARTED[@]} -eq 0 ]] && die "no personas could start"

# Device sampling while load is running: RSS early + late (take the max), one
# CPU window across the middle of the run.
sleep 3
pi_mem > "$RUN_DIR/device/load-mem-1.json" || true
CPU_WIN=$(( DURATION - 12 )); (( CPU_WIN > 30 )) && CPU_WIN=30; (( CPU_WIN < 5 )) && CPU_WIN=5
pi_cpu "$CPU_WIN" > "$RUN_DIR/device/load-cpu.json" || log "WARN: load CPU sample failed"
pi_mem > "$RUN_DIR/device/load-mem-2.json" || true

wait ${PERSONA_PIDS[@]+"${PERSONA_PIDS[@]}"}
PERSONA_PIDS=()
log "All personas finished"

# ---------------------------------------------------------------------------
# Step 8: build summary.json
# ---------------------------------------------------------------------------
# Per-persona row from oha JSON. Latencies in ms. "aborted due to deadline" is
# normally just the requests in flight when -z expires (not a server error) —
# BUT when it equals the connection count AND the persona missed its target
# rate, every connection was wedged on a response that never completed; the
# reading below calls that signature out (found in practice: streaming SSR
# bodies that stall under concurrent sfn load).
ROWS_FILE="$RUN_DIR/persona-rows.json"
{
    for spec in "${PERSONAS[@]}"; do
        IFS='|' read -r name conc qps kind target body <<<"$spec"
        f="$RUN_DIR/personas/$name.json"
        if [[ ! -s "$f" ]]; then
            note="skipped: server fn not on this build"
            [[ " ${SKIPPED[*]-} " != *" $name "* ]] && note="failed: $(head -c200 "$RUN_DIR/personas/$name.err" 2>/dev/null)"
            jq -n --arg name "$name" --arg note "$note" \
                '{key: $name, value: {skipped: true, note: $note}}'
            continue
        fi
        jq --arg name "$name" --arg conc "$conc" --arg qps "$qps" --arg kind "$kind" '
            {key: $name, value: {
                skipped: false, kind: $kind,
                connections: ($conc|tonumber), target_rps: ($qps|tonumber),
                requests: (.statusCodeDistribution | add // 0),
                rps: .summary.requestsPerSec,
                p50_ms: (.latencyPercentiles.p50 * 1000),
                p90_ms: (.latencyPercentiles.p90 * 1000),
                p99_ms: (.latencyPercentiles.p99 * 1000),
                worst_ms: (.summary.slowest * 1000),
                non2xx: ([.statusCodeDistribution | to_entries[]
                          | select(.key | startswith("2") | not) | .value] | add // 0),
                transport_errors: ([(.errorDistribution // {}) | to_entries[]
                          | select(.key != "aborted due to deadline") | .value] | add // 0),
                aborted_at_deadline: ((.errorDistribution // {})["aborted due to deadline"] // 0),
                status_codes: .statusCodeDistribution
            }}' "$f"
    done
} | jq -s 'from_entries' > "$ROWS_FILE"

# Device metrics: prefer the larger of the two under-load RSS samples.
DEVICE_JSON=$(jq -n \
    --slurpfile im <(cat "$RUN_DIR/device/idle-mem.json" 2>/dev/null || echo '{}') \
    --slurpfile ic <(cat "$RUN_DIR/device/idle-cpu.json" 2>/dev/null || echo '{}') \
    --slurpfile l1 <(cat "$RUN_DIR/device/load-mem-1.json" 2>/dev/null || echo '{}') \
    --slurpfile l2 <(cat "$RUN_DIR/device/load-mem-2.json" 2>/dev/null || echo '{}') \
    --slurpfile lc <(cat "$RUN_DIR/device/load-cpu.json" 2>/dev/null || echo '{}') '
    {
        idle_rss_kb: ($im[0].vmrss_kb // null),
        load_rss_kb: ([($l1[0].vmrss_kb // 0), ($l2[0].vmrss_kb // 0)] | max
                      | if . == 0 then null else . end),
        idle_cpu_pct_one_core: ($ic[0].proc_pct_one_core // null),
        load_cpu_pct_one_core: ($lc[0].proc_pct_one_core // null),
        idle_host_pct: ($ic[0].host_pct // null),
        load_host_pct: ($lc[0].host_pct // null),
        ncpu: ($lc[0].ncpu // null),
        load_loadavg: ($lc[0].loadavg // null)
    }')

LOCAL_HEAD=$(git -C "$REPO_ROOT" rev-parse --short HEAD 2>/dev/null || echo unknown)
jq -n \
    --arg ts "$TS" --arg base "$BASE" --arg app_version "$APP_VERSION" \
    --arg app_git_hash "$APP_GIT_HASH" --arg local_head "$LOCAL_HEAD" \
    --arg oha_version "$(oha --version | awk '{print $2}')" \
    --arg mode "$MODE" --arg storage "$STORAGE_KIND" \
    --arg grid_system "$GRID_SYSTEM" --arg detail_game "$DETAIL_GAME" \
    --arg readiness_note "$READINESS_NOTE" \
    --argjson duration "$DURATION" --argjson total_roms "$TOTAL_ROMS" \
    --argjson deep_offset "$DEEP_OFFSET" \
    --argjson device "$DEVICE_JSON" --slurpfile personas "$ROWS_FILE" '
    {
        meta: {
            timestamp: $ts, base_url: $base,
            app_version: $app_version, app_git_hash: $app_git_hash,
            local_head: $local_head, oha_version: $oha_version,
            mode: $mode, duration_s: $duration,
            total_roms: $total_roms, storage_kind: $storage,
            grid_system: $grid_system, deep_offset: $deep_offset,
            detail_game: $detail_game, readiness_note: $readiness_note
        },
        device: $device,
        personas: $personas[0]
    }' > "$RUN_DIR/summary.json"

# ---------------------------------------------------------------------------
# Step 9: human-readable summary.md with an auto-generated "reading"
# ---------------------------------------------------------------------------
READING=$(jq -r '
    def flag(cond; msg): if cond then msg else empty end;
    [ (.personas | to_entries[] | select(.value.skipped == false) | .key as $k | .value as $v |
        ( flag($v.p99_ms > 10 * $v.p50_ms and $v.requests >= 20;
            "**\($k)**: heavy tail — p99 (\($v.p99_ms|round)ms) is >10x p50 (\($v.p50_ms|round)ms)"),
          flag($v.non2xx > 0;
            "**\($k)**: \($v.non2xx) non-2xx responses (\($v.status_codes))"),
          flag($v.transport_errors > 0;
            "**\($k)**: \($v.transport_errors) transport errors"),
          flag($v.rps < 0.7 * $v.target_rps and $v.requests >= 20
               and $v.aborted_at_deadline >= $v.connections;
            "**\($k)**: only \($v.rps*100/$v.target_rps|round)% of target rate and ALL \($v.connections) connections were wedged at the deadline while completed-request latencies stayed low — some responses never complete (stalled streaming SSR bodies); latency percentiles are survivor-biased"),
          flag($v.rps < 0.7 * $v.target_rps and $v.requests >= 20
               and $v.aborted_at_deadline < $v.connections;
            "**\($k)**: achieved \($v.rps*100/$v.target_rps|round)% of its target rate — the server could not keep up")
        )),
      (.personas | to_entries[] | select(.value.skipped == true) |
          "**\(.key)**: \(.value.note)"),
      (flag(.device.load_host_pct != null and (.device.load_host_pct|tonumber) > 85;
          "device host CPU at \(.device.load_host_pct)% during load — saturated")),
      (flag(.device.load_cpu_pct_one_core != null and .device.ncpu != null
            and (.device.load_cpu_pct_one_core|tonumber) > 80 * (.device.ncpu|tonumber);
          "app process near full-machine CPU during load")),
      (flag(.device.idle_rss_kb != null and .device.load_rss_kb != null
            and .device.load_rss_kb > 1.2 * .device.idle_rss_kb;
          "RSS grew \((.device.load_rss_kb - .device.idle_rss_kb)/1024|round)MB (>20%) under load — likely cache warm-up on first traffic; a leak only if it keeps growing run-over-run")),
      (if (.personas.scroller_deep.skipped != false) or (.personas.scroller_shallow.skipped != false) then empty
       elif .personas.scroller_deep.p50_ms > 3 * .personas.scroller_shallow.p50_ms then
          "deep scroll (offset \(.meta.deep_offset)) p50 is \(.personas.scroller_deep.p50_ms / .personas.scroller_shallow.p50_ms | round)x shallow p50 — OFFSET pagination cost is visible"
       else empty end)
    ] | if length == 0 then "- No anomalies: all personas within expected latency shape, no errors, device not saturated."
        else map("- " + .) | join("\n") end
' "$RUN_DIR/summary.json")

{
    echo "# User-load run $TS"
    echo
    jq -r '.meta | "- **Target**: \(.base_url) — app v\(.app_version) (\(.app_git_hash)), storage: \(.storage_kind), \(.total_roms) games
- **Mode**: \(.mode), \(.duration_s)s measurement, oha \(.oha_version)
- **Grid system**: \(.grid_system) (deep offset \(.deep_offset)); detail game: \(.detail_game)"' "$RUN_DIR/summary.json"
    [[ -n "$READINESS_NOTE" ]] && echo "- **Note**: $READINESS_NOTE"
    echo
    echo "## Personas"
    echo
    echo "| persona | req | rps | p50 ms | p90 ms | p99 ms | worst ms | non-2xx | errors |"
    echo "|---|---|---|---|---|---|---|---|---|"
    jq -r '.personas | to_entries[] |
        if .value.skipped then "| \(.key) | — | — | — | — | — | — | — | skipped |"
        else "| \(.key) | \(.value.requests) | \(.value.rps*100|round/100) | \(.value.p50_ms*10|round/10) | \(.value.p90_ms*10|round/10) | \(.value.p99_ms*10|round/10) | \(.value.worst_ms|round) | \(.value.non2xx) | \(.value.transport_errors) |"
        end' "$RUN_DIR/summary.json"
    echo
    echo "## Device (Pi)"
    echo
    jq -r '.device | "| metric | idle | under load |
|---|---|---|
| app RSS | \(if .idle_rss_kb then "\(.idle_rss_kb/1024|round) MB" else "n/a" end) | \(if .load_rss_kb then "\(.load_rss_kb/1024|round) MB" else "n/a" end) |
| app CPU (% of one core) | \(.idle_cpu_pct_one_core // "n/a") | \(.load_cpu_pct_one_core // "n/a") |
| host CPU % | \(.idle_host_pct // "n/a") | \(.load_host_pct // "n/a") |"' "$RUN_DIR/summary.json"
    echo
    echo "## Reading"
    echo
    echo "$READING"
    echo
    echo "_Noise floor: this Pi shows +/-5-7% run-to-run variance on identical builds;_"
    echo "_treat sub-10% deltas as noise. Raw oha JSON per persona sits next to this file._"
} > "$RUN_DIR/summary.md"

log "Results: $RUN_DIR/summary.md"
echo
cat "$RUN_DIR/summary.md"

# ---------------------------------------------------------------------------
# Step 10: baseline compare (regression gate)
# ---------------------------------------------------------------------------
# Auto-compare against "main" when it exists and nothing was requested.
if [[ -z "$COMPARE" && "$NO_COMPARE" == false && -f "$BASELINE_DIR/main.json" ]]; then
    COMPARE="main"
fi

EXIT_CODE=0
if [[ -n "$COMPARE" ]]; then
    BASE_FILE="$BASELINE_DIR/$COMPARE.json"
    if [[ ! -f "$BASE_FILE" ]]; then
        log "WARN: baseline '$COMPARE' not found at $BASE_FILE — skipping comparison"
    else
        CMP=$(jq -n \
            --slurpfile b "$BASE_FILE" --slurpfile c "$RUN_DIR/summary.json" \
            --argjson th_rps "$TH_RPS_DROP" --argjson th_lat "$TH_LAT_UP" --argjson th_rss "$TH_RSS_UP" '
            def pct(cur; base): if base == null or base == 0 or cur == null then null
                                else (cur - base) * 100 / base end;
            def fmt(p): if p == null then "n/a" elif p >= 0 then "+\(p*10|round/10)%" else "\(p*10|round/10)%" end;
            ($b[0]) as $B | ($c[0]) as $C |
            # Cross-version comparison is a primary use case: headline it.
            (if $B.meta.app_version != $C.meta.app_version
             then "v\($B.meta.app_version) -> v\($C.meta.app_version)"
             else "v\($C.meta.app_version) (same version)" end) as $headline |
            ([ (if ($B.meta.total_roms != $C.meta.total_roms) then
                    "library size differs: \($B.meta.total_roms) vs \($C.meta.total_roms) games — results NOT comparable" else empty end),
               (if ($B.meta.storage_kind != $C.meta.storage_kind) then
                    "storage differs: \($B.meta.storage_kind) vs \($C.meta.storage_kind) — results NOT comparable" else empty end),
               (if ($B.meta.mode != $C.meta.mode) then
                    "keepalive mode differs: \($B.meta.mode) vs \($C.meta.mode) — results NOT comparable" else empty end),
               (if ($B.meta.duration_s != $C.meta.duration_s) then
                    "duration differs: \($B.meta.duration_s)s vs \($C.meta.duration_s)s — tail percentiles less comparable" else empty end)
             ]) as $warnings |
            ([ $C.personas | to_entries[] | .key as $name |
               ($B.personas[$name]) as $bp | .value as $cp |
               select($bp != null and ($bp.skipped | not) and ($cp.skipped | not)) |
               {
                   persona: $name,
                   d_rps: pct($cp.rps; $bp.rps),
                   d_p50: pct($cp.p50_ms; $bp.p50_ms),
                   d_p99: pct($cp.p99_ms; $bp.p99_ms),
                   base_err: ($bp.non2xx + $bp.transport_errors),
                   cur_err: ($cp.non2xx + $cp.transport_errors),
                   row: "| \($name) | \($bp.rps*100|round/100) -> \($cp.rps*100|round/100) (\(fmt(pct($cp.rps; $bp.rps)))) | \($bp.p50_ms|round) -> \($cp.p50_ms|round) (\(fmt(pct($cp.p50_ms; $bp.p50_ms)))) | \($bp.p99_ms|round) -> \($cp.p99_ms|round) (\(fmt(pct($cp.p99_ms; $bp.p99_ms)))) | \($bp.non2xx + $bp.transport_errors) -> \($cp.non2xx + $cp.transport_errors) |"
               } ]) as $rows |
            ([ $rows[] |
               (if .d_rps != null and .d_rps < -$th_rps then ["\(.persona): rps \(fmt(.d_rps)) (threshold -\($th_rps)%)"] else [] end),
               (if .d_p50 != null and .d_p50 > $th_lat then ["\(.persona): p50 \(fmt(.d_p50)) (threshold +\($th_lat)%)"] else [] end),
               (if .d_p99 != null and .d_p99 > $th_lat then ["\(.persona): p99 \(fmt(.d_p99)) (threshold +\($th_lat)%)"] else [] end),
               (if .cur_err > .base_err then ["\(.persona): errors \(.base_err) -> \(.cur_err) (new errors are always a regression)"] else [] end)
             | .[] ]) as $persona_regr |
            (pct($C.device.load_rss_kb; $B.device.load_rss_kb)) as $d_rss |
            (pct($C.device.load_cpu_pct_one_core | tonumber?; $B.device.load_cpu_pct_one_core | tonumber?)) as $d_cpu |
            ($persona_regr
             + (if $d_rss != null and $d_rss > $th_rss then ["device: load RSS \(fmt($d_rss)) (threshold +\($th_rss)%)"] else [] end)
            ) as $regressions |
            {
                regressions: ($regressions | length),
                text: ([
                    "## Comparison vs baseline \"'"$COMPARE"'\": \($headline)",
                    "",
                    ($warnings | map("- WARNING: " + .) | join("\n")),
                    (if ($warnings | length) > 0 then "" else empty end),
                    "| persona | rps (base -> cur) | p50 ms | p99 ms | errors |",
                    "|---|---|---|---|---|",
                    ($rows | map(.row) | join("\n")),
                    "",
                    "Device: load RSS \(fmt($d_rss)), load CPU \(fmt($d_cpu)) vs baseline.",
                    "",
                    (if ($regressions | length) > 0
                     then "**VERDICT: REGRESSION** — " + ($regressions | join("; "))
                     else "**VERDICT: OK** — all deltas within thresholds (noise floor is +/-5-7%)." end)
                ] | join("\n"))
            }')
        echo
        jq -r '.text' <<<"$CMP" | tee -a "$RUN_DIR/summary.md"
        [[ "$(jq -r '.regressions' <<<"$CMP")" -gt 0 ]] && EXIT_CODE=1
    fi
fi

# ---------------------------------------------------------------------------
# Step 11: save baseline if requested
# ---------------------------------------------------------------------------
if [[ -n "$SAVE_BASELINE" ]]; then
    cp "$RUN_DIR/summary.json" "$BASELINE_DIR/$SAVE_BASELINE.json"
    log "Saved baseline '$SAVE_BASELINE' -> $BASELINE_DIR/$SAVE_BASELINE.json"
fi

exit "$EXIT_CODE"
