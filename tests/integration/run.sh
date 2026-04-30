#!/usr/bin/env bash
#
# Integration test runner for replay-control.
#
# Builds and runs the app in a network-isolated container, then verifies
# HTTP responses with curl-based tests.
#
# Usage:
#   ./tests/integration/run.sh              # auto-detect podman/docker
#   CONTAINER_ENGINE=docker ./tests/integration/run.sh
#
# Prerequisites:
#   1. Build the app:         ./build.sh
#   2. Generate fixtures:     cargo run -p generate-test-fixtures
#   3. Container engine:      podman or docker installed

set -euo pipefail

# ── Configuration ─────────────────────────────────────────────────────────────

IMAGE_NAME="replay-control-test"
CONTAINER_NAME="replay-control-test-run"
HOST_PORT="${TEST_PORT:-8080}"
CONTAINER_PORT="8080"
MAX_WAIT=30  # seconds to wait for healthcheck

# ── Detect container engine ───────────────────────────────────────────────────

if [[ -n "${CONTAINER_ENGINE:-}" ]]; then
    ENGINE="$CONTAINER_ENGINE"
elif command -v podman &>/dev/null; then
    ENGINE="podman"
elif command -v docker &>/dev/null; then
    ENGINE="docker"
else
    echo "ERROR: No container engine found. Install podman or docker."
    exit 1
fi

echo "Using container engine: $ENGINE"

# ── Resolve paths ─────────────────────────────────────────────────────────────

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

cd "$REPO_ROOT"

# ── Validate prerequisites ────────────────────────────────────────────────────

# Find the app binary
APP_BIN=""
if [[ -f "target/release/replay-control-app" ]]; then
    APP_BIN="target/release/replay-control-app"
elif [[ -f "target/x86_64-unknown-linux-gnu/release/replay-control-app" ]]; then
    APP_BIN="target/x86_64-unknown-linux-gnu/release/replay-control-app"
else
    echo "ERROR: App binary not found. Run ./build.sh first."
    exit 1
fi

if [[ ! -d "target/site" ]]; then
    echo "ERROR: Site assets not found at target/site/. Run ./build.sh first."
    exit 1
fi

if [[ ! -d "tests/fixtures/storage/roms" ]]; then
    echo "ERROR: Test fixtures not found. Run: cargo run -p generate-test-fixtures"
    exit 1
fi

echo "App binary: $APP_BIN"

# ── Build container image ─────────────────────────────────────────────────────

echo ""
echo "Building container image..."
$ENGINE build \
    -f Containerfile.test \
    --build-arg "APP_BIN=$APP_BIN" \
    -t "$IMAGE_NAME" \
    .

# ── Cleanup any previous run ─────────────────────────────────────────────────

$ENGINE rm -f "$CONTAINER_NAME" 2>/dev/null || true

# ── Run container (network-isolated) ─────────────────────────────────────────

echo ""
echo "Starting container (network=none, port=$HOST_PORT)..."
$ENGINE run -d \
    --name "$CONTAINER_NAME" \
    --network=none \
    -p "$HOST_PORT:$CONTAINER_PORT" \
    "$IMAGE_NAME"

# ── Wait for healthcheck ─────────────────────────────────────────────────────

echo "Waiting for app to start..."
READY=false
for i in $(seq 1 "$MAX_WAIT"); do
    if curl -sf "http://localhost:$HOST_PORT/" >/dev/null 2>&1; then
        READY=true
        echo "  App ready after ${i}s"
        break
    fi
    sleep 1
done

if [[ "$READY" != "true" ]]; then
    echo "ERROR: App did not start within ${MAX_WAIT}s"
    echo ""
    echo "Container logs:"
    $ENGINE logs "$CONTAINER_NAME" 2>&1 | tail -30
    $ENGINE rm -f "$CONTAINER_NAME" 2>/dev/null || true
    exit 1
fi

# ── Test functions ────────────────────────────────────────────────────────────

PASS=0
FAIL=0
BASE="http://localhost:$HOST_PORT"

assert_status() {
    local desc="$1" url="$2" expected_status="$3"
    local status
    status=$(curl -s -o /dev/null -w "%{http_code}" "$url" 2>/dev/null || echo "000")
    if [[ "$status" == "$expected_status" ]]; then
        echo "  PASS: $desc (HTTP $status)"
        PASS=$((PASS + 1))
    else
        echo "  FAIL: $desc — expected $expected_status, got $status"
        FAIL=$((FAIL + 1))
    fi
}

assert_contains() {
    local desc="$1" url="$2" expected_text="$3"
    local body
    body=$(curl -sf "$url" 2>/dev/null || echo "")
    if echo "$body" | grep -qi "$expected_text"; then
        echo "  PASS: $desc"
        PASS=$((PASS + 1))
    else
        echo "  FAIL: $desc — response does not contain '$expected_text'"
        FAIL=$((FAIL + 1))
    fi
}

# ── Run tests ─────────────────────────────────────────────────────────────────

echo ""
echo "Running integration tests..."
echo ""

# 1. Home page
echo "--- Home page ---"
assert_status "Home page returns 200" "$BASE/" "200"
assert_contains "Home page has app title" "$BASE/" "Replay Control"

# 2. System (games) pages — the route is /games/<system>, NOT /system/<system>.
# /system/* falls through to the leptos route fallback ("Page not found")
# and still returns HTTP 200; that's why an earlier `assert_status … 200`
# style here was passing while the actual page was broken. We anchor the
# real route by checking content (the system display name + filter UI),
# not just status.
echo ""
echo "--- Games pages ---"
assert_status     "SNES games page returns 200"   "$BASE/games/nintendo_snes" "200"
assert_status     "Genesis games page returns 200" "$BASE/games/sega_smd"     "200"
assert_status     "Arcade games page returns 200" "$BASE/games/arcade_fbneo" "200"
assert_status     "N64 games page returns 200"    "$BASE/games/nintendo_n64" "200"
assert_contains   "Games page is not the 404 fallback" "$BASE/games/nintendo_snes" "Hide Hacks"
assert_contains   "Games page mentions filter UI" "$BASE/games/nintendo_snes" "All Genres"

# 3. Search
echo ""
echo "--- Search ---"
assert_status "Search page returns 200" "$BASE/search?q=mario" "200"
assert_contains "Search finds Mario games" "$BASE/search?q=mario" "Mario"
assert_status "Search with no results returns 200" "$BASE/search?q=zzzznonexistent" "200"

# 4. Favorites
echo ""
echo "--- Favorites ---"
assert_status "Favorites page returns 200" "$BASE/favorites" "200"

# 5. Settings
echo ""
echo "--- Settings ---"
assert_status "Settings page returns 200" "$BASE/settings" "200"
assert_status "Metadata page returns 200" "$BASE/settings/metadata" "200"
assert_contains "Metadata page mentions Built-in" "$BASE/settings/metadata" "Built"

# Tier 1 snapshot warm-cache check: a second request should be markedly
# faster than the first because the metadata-page snapshot stays warm
# until something invalidates it.
echo "--- Metadata snapshot warm cache ---"
COLD=$(curl -s -o /dev/null -w "%{time_total}" "$BASE/settings/metadata" 2>/dev/null || echo "0")
WARM=$(curl -s -o /dev/null -w "%{time_total}" "$BASE/settings/metadata" 2>/dev/null || echo "0")
echo "  cold=${COLD}s warm=${WARM}s (informational)"

# 6. Non-existent routes
echo ""
echo "--- Error handling ---"
# Leptos returns 200 for any URL but renders a "Page not found" body for
# unknown routes. Anchor the negative case via content, not status.
assert_contains "Unknown route renders fallback" "$BASE/games/nonexistent_xyz" "Page not found"

# 7. New server fns from the pool-design work — make sure they're
# registered and reachable. These are POST endpoints (server fn calls).
echo ""
echo "--- Server fns wired ---"
GMS_STATUS=$(curl -s -o /dev/null -w "%{http_code}" -X POST -H 'Content-Type: application/json' \
    "$BASE/sfn/GetMetadataPageSnapshot" -d '{}' 2>/dev/null || echo "000")
if [[ "$GMS_STATUS" =~ ^(200|400|405)$ ]]; then
    # 200 = success; 400 = bad request body (still wired); 405 = method allowed list (still wired).
    echo "  PASS: GetMetadataPageSnapshot is registered (HTTP $GMS_STATUS)"
    PASS=$((PASS + 1))
else
    echo "  FAIL: GetMetadataPageSnapshot not reachable (HTTP $GMS_STATUS)"
    FAIL=$((FAIL + 1))
fi

# ── Cleanup ───────────────────────────────────────────────────────────────────

echo ""
$ENGINE stop "$CONTAINER_NAME" >/dev/null 2>&1 || true
$ENGINE rm -f "$CONTAINER_NAME" >/dev/null 2>&1 || true

# ── Summary ───────────────────────────────────────────────────────────────────

TOTAL=$((PASS + FAIL))
echo "================================"
echo "Results: $PASS/$TOTAL passed"
if [[ $FAIL -gt 0 ]]; then
    echo "         $FAIL FAILED"
    echo "================================"
    exit 1
else
    echo "         All tests passed!"
    echo "================================"
    exit 0
fi
