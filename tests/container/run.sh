#!/usr/bin/env bash
# Engine-agnostic container test runner.
#
# Builds the app, builds the container image, starts it, runs Playwright
# tests, and tears down. Works with both Podman and Docker.
#
# Usage:
#   ./tests/container/run.sh
#
# Environment variables:
#   CONTAINER_ENGINE  - "podman" or "docker" (auto-detected if unset)
#   SKIP_BUILD        - set to "1" to skip app build (use existing artifacts)
#   MOCK_PORT         - port for mock GitHub server (default: 9999)
#   APP_PORT          - port to expose the app on (default: 8080)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

# --- Engine detection ---
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

MOCK_PORT="${MOCK_PORT:-9999}"
APP_PORT="${APP_PORT:-8080}"
IMAGE_NAME="replay-control-replayos"
CONTAINER_NAME="replay-control-test-$$"

# Host gateway for the container to reach the mock server
if [[ "$ENGINE" == "podman" ]]; then
    HOST_GATEWAY="host.containers.internal"
else
    HOST_GATEWAY="host.docker.internal"
fi

# --- Cleanup on exit ---
cleanup() {
    echo "Cleaning up..."
    # Stop mock server
    if [[ -n "${MOCK_PID:-}" ]]; then
        kill "$MOCK_PID" 2>/dev/null || true
        wait "$MOCK_PID" 2>/dev/null || true
    fi
    # Stop and remove container
    $ENGINE rm -f "$CONTAINER_NAME" 2>/dev/null || true
}
trap cleanup EXIT

# --- Step 1: Build the app ---
if [[ "${SKIP_BUILD:-}" != "1" ]]; then
    echo "Building x86_64 binary + WASM + site assets..."
    cd "$PROJECT_ROOT"
    ./build.sh
else
    echo "Skipping build (SKIP_BUILD=1)"
fi

# --- Step 2: Build the container image ---
echo "Building container image..."
cd "$PROJECT_ROOT"
$ENGINE build -f Containerfile.replayos -t "$IMAGE_NAME" .

# --- Step 3: Start mock GitHub server ---
echo "Starting mock GitHub server on port $MOCK_PORT..."
python3 "$SCRIPT_DIR/mock_github.py" --port "$MOCK_PORT" &
MOCK_PID=$!

# Wait for mock server to be ready
for i in $(seq 1 10); do
    if curl -sf "http://localhost:$MOCK_PORT/health" >/dev/null 2>&1; then
        echo "Mock server ready."
        break
    fi
    if [[ $i -eq 10 ]]; then
        echo "ERROR: Mock server failed to start."
        exit 1
    fi
    sleep 0.5
done

# --- Step 4: Start the container ---
echo "Starting container..."
$ENGINE run -d \
    --name "$CONTAINER_NAME" \
    -p "$APP_PORT:8080" \
    -e "REPLAY_GITHUB_API_URL=http://$HOST_GATEWAY:$MOCK_PORT" \
    "$IMAGE_NAME"

# --- Step 5: Wait for health check ---
echo "Waiting for app to be ready..."
for i in $(seq 1 30); do
    if curl -sf "http://localhost:$APP_PORT/api/version" >/dev/null 2>&1; then
        echo "App is ready on port $APP_PORT."
        break
    fi
    if [[ $i -eq 30 ]]; then
        echo "ERROR: App failed to start. Container logs:"
        $ENGINE logs "$CONTAINER_NAME"
        exit 1
    fi
    sleep 1
done

# --- Step 6: Run Playwright tests ---
echo "Running Playwright tests..."
cd "$PROJECT_ROOT"

RESULT=0
APP_URL="http://localhost:$APP_PORT" \
    python3 -m pytest tests/e2e/ -v --timeout=120 || RESULT=$?

# --- Step 7: Report results ---
if [[ $RESULT -eq 0 ]]; then
    echo ""
    echo "All container tests passed."
else
    echo ""
    echo "Container tests FAILED (exit code: $RESULT)."
    echo "Container logs:"
    $ENGINE logs "$CONTAINER_NAME" 2>&1 | tail -50
fi

exit $RESULT
