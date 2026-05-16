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
#   MOCK_HOST         - host/address the app container uses for the mock server
#   APP_PORT          - port to expose the app on, or 0 for automatic (default: 8080)
#   APP_HOST          - host/address used by readiness + Playwright (default: 127.0.0.1)
#   CONTAINER_NETWORK - optional container network mode (e.g. bridge)
#   PODMAN_DIRECT_BRIDGE - set to "1" to run app + mock on podman's bridge
#                          and drive tests through container IPs. Useful for
#                          rootless Podman setups where published ports refuse
#                          host connections.
#   PODMAN_MOCK_IP    - static mock IP for PODMAN_DIRECT_BRIDGE
#                          (default: 10.88.250.<derived>)
#   MOCK_BASE_IMAGE   - Python-capable base image for the bridge mock image
#                          (default: local code-server image if present,
#                          otherwise python:3.12-slim)
#   PYTEST_ARGS       - pytest args to run (default: tests/e2e/ -v)

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
APP_HOST="${APP_HOST:-127.0.0.1}"
IMAGE_NAME="replay-control-replayos"
CONTAINER_NAME="replay-control-test-$$"
MOCK_CONTAINER_NAME="replay-control-mock-$$"
MOCK_IMAGE_NAME="replay-control-mock-github:e2e"
BUILD_CONTEXT=""
MOCK_BUILD_CONTEXT=""
DIRECT_BRIDGE=false
APP_URL=""
APP_MOCK_URL=""
MOCK_CONTROL_URL=""

if [[ "$ENGINE" == "podman" && "${PODMAN_DIRECT_BRIDGE:-}" == "1" ]]; then
    DIRECT_BRIDGE=true
    APP_HOST=""
    APP_PORT="8080"
fi

# Host gateway for the container to reach the mock server
if [[ "$ENGINE" == "podman" ]]; then
    HOST_GATEWAY="host.containers.internal"
else
    HOST_GATEWAY="host.docker.internal"
fi
if [[ "$DIRECT_BRIDGE" == "true" ]]; then
    MOCK_HOST=""
elif [[ -z "${MOCK_HOST:-}" ]]; then
    if [[ "$ENGINE" == "podman" && "${CONTAINER_NETWORK:-}" == "bridge" ]]; then
        MOCK_HOST="$(hostname -I 2>/dev/null | awk '{ print $1 }')"
        MOCK_HOST="${MOCK_HOST:-$HOST_GATEWAY}"
    else
        MOCK_HOST="$HOST_GATEWAY"
    fi
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
    $ENGINE rm -f "$MOCK_CONTAINER_NAME" 2>/dev/null || true
    # Remove staged container build context
    if [[ -n "$BUILD_CONTEXT" ]]; then
        rm -rf "$BUILD_CONTEXT"
    fi
    if [[ -n "$MOCK_BUILD_CONTEXT" ]]; then
        rm -rf "$MOCK_BUILD_CONTEXT"
    fi
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
TARGET_DIR="${CARGO_TARGET_DIR:-target}"
if [[ "$TARGET_DIR" != /* ]]; then
    TARGET_DIR="$PROJECT_ROOT/$TARGET_DIR"
fi

APP_BINARY="$TARGET_DIR/release/replay-control-app"
SITE_DIR="$TARGET_DIR/site"
for required_path in "$APP_BINARY" "$SITE_DIR" "$PROJECT_ROOT/catalog.sqlite"; do
    if [[ ! -e "$required_path" ]]; then
        echo "ERROR: required build artifact missing: $required_path"
        exit 1
    fi
done

BUILD_CONTEXT="$(mktemp -d "${TMPDIR:-/tmp}/replay-control-container.XXXXXX")"
mkdir -p \
    "$BUILD_CONTEXT/target/release" \
    "$BUILD_CONTEXT/tests/container/fixtures"
cp "$PROJECT_ROOT/Containerfile.replayos" "$BUILD_CONTEXT/Containerfile.replayos"
cp "$PROJECT_ROOT/tests/container/mock_systemctl.sh" "$BUILD_CONTEXT/tests/container/mock_systemctl.sh"
cp "$PROJECT_ROOT/tests/container/fixtures/replay.cfg" "$BUILD_CONTEXT/tests/container/fixtures/replay.cfg"
cp "$PROJECT_ROOT/tests/container/fixtures/environment" "$BUILD_CONTEXT/tests/container/fixtures/environment"
cp "$APP_BINARY" "$BUILD_CONTEXT/target/release/replay-control-app"
cp -R "$SITE_DIR" "$BUILD_CONTEXT/target/site"
cp "$PROJECT_ROOT/catalog.sqlite" "$BUILD_CONTEXT/catalog.sqlite"

$ENGINE build -f "$BUILD_CONTEXT/Containerfile.replayos" -t "$IMAGE_NAME" "$BUILD_CONTEXT"

# --- Step 3: Start mock GitHub server ---
if [[ "$DIRECT_BRIDGE" == "true" ]]; then
    PODMAN_MOCK_IP="${PODMAN_MOCK_IP:-10.88.250.$(( ($$ % 200) + 20 ))}"
    if [[ -z "${MOCK_BASE_IMAGE:-}" ]]; then
        if $ENGINE image exists localhost/code-server:4.116.0; then
            MOCK_BASE_IMAGE="localhost/code-server:4.116.0"
        else
            MOCK_BASE_IMAGE="python:3.12-slim"
        fi
    fi

    echo "Building mock GitHub image from $MOCK_BASE_IMAGE..."
    MOCK_BUILD_CONTEXT="$(mktemp -d "${TMPDIR:-/tmp}/replay-control-mock.XXXXXX")"
    mkdir -p \
        "$MOCK_BUILD_CONTEXT/tests/container" \
        "$MOCK_BUILD_CONTEXT/replay-control-app"
    cp "$PROJECT_ROOT/tests/container/mock_github.py" \
        "$MOCK_BUILD_CONTEXT/tests/container/mock_github.py"
    cp "$PROJECT_ROOT/replay-control-app/Cargo.toml" \
        "$MOCK_BUILD_CONTEXT/replay-control-app/Cargo.toml"
    {
        printf 'FROM %s\n' "$MOCK_BASE_IMAGE"
        printf 'WORKDIR /work\n'
        printf 'COPY tests/container/mock_github.py tests/container/mock_github.py\n'
        printf 'COPY replay-control-app/Cargo.toml replay-control-app/Cargo.toml\n'
        printf 'ENTRYPOINT ["python3", "tests/container/mock_github.py"]\n'
    } > "$MOCK_BUILD_CONTEXT/Containerfile"
    $ENGINE build -f "$MOCK_BUILD_CONTEXT/Containerfile" \
        -t "$MOCK_IMAGE_NAME" "$MOCK_BUILD_CONTEXT"

    echo "Starting mock GitHub container at $PODMAN_MOCK_IP:$MOCK_PORT..."
    $ENGINE run -d \
        --network podman \
        --ip "$PODMAN_MOCK_IP" \
        --name "$MOCK_CONTAINER_NAME" \
        -e "MOCK_GITHUB_PUBLIC_BASE_URL=http://$PODMAN_MOCK_IP:$MOCK_PORT" \
        "$MOCK_IMAGE_NAME" \
        --port "$MOCK_PORT"
    MOCK_CONTROL_URL="http://$PODMAN_MOCK_IP:$MOCK_PORT"
    APP_MOCK_URL="$MOCK_CONTROL_URL"

    for i in $(seq 1 20); do
        if curl -sf "$MOCK_CONTROL_URL/health" >/dev/null 2>&1; then
            echo "Mock server ready at $MOCK_CONTROL_URL."
            break
        fi
        if [[ $i -eq 20 ]]; then
            echo "ERROR: Mock server failed to start. Container logs:"
            $ENGINE logs "$MOCK_CONTAINER_NAME"
            exit 1
        fi
        sleep 0.5
    done
else
    echo "Starting mock GitHub server on port $MOCK_PORT..."
    MOCK_GITHUB_PUBLIC_BASE_URL="http://$MOCK_HOST:$MOCK_PORT" \
        python3 "$SCRIPT_DIR/mock_github.py" --port "$MOCK_PORT" &
    MOCK_PID=$!
    MOCK_CONTROL_URL="http://127.0.0.1:$MOCK_PORT"
    APP_MOCK_URL="http://$MOCK_HOST:$MOCK_PORT"

    # Wait for mock server to be ready
    for i in $(seq 1 10); do
        if ! kill -0 "$MOCK_PID" 2>/dev/null; then
            echo "ERROR: Mock server exited before becoming ready."
            wait "$MOCK_PID" 2>/dev/null || true
            exit 1
        fi
        if curl -sf "$MOCK_CONTROL_URL/health" >/dev/null 2>&1; then
            echo "Mock server ready."
            break
        fi
        if [[ $i -eq 10 ]]; then
            echo "ERROR: Mock server failed to start."
            exit 1
        fi
        sleep 0.5
    done
fi

# --- Step 4: Start the container ---
echo "Starting container..."
EXTRA_ARGS=()
if [[ "$ENGINE" == "docker" ]]; then
    EXTRA_ARGS+=(--add-host=host.docker.internal:host-gateway)
fi
if [[ -n "${CONTAINER_NETWORK:-}" ]]; then
    EXTRA_ARGS+=(--network "$CONTAINER_NETWORK")
fi
PUBLISH_ARGS=()
if [[ "$DIRECT_BRIDGE" == "true" ]]; then
    EXTRA_ARGS+=(--network podman)
elif [[ "$APP_PORT" == "0" ]]; then
    PUBLISH_ARGS+=(-p "8080")
else
    PUBLISH_ARGS+=(-p "$APP_PORT:8080")
fi

$ENGINE run -d \
    --name "$CONTAINER_NAME" \
    "${PUBLISH_ARGS[@]}" \
    -e "REPLAY_GITHUB_API_URL=${APP_MOCK_URL}" \
    "${EXTRA_ARGS[@]}" \
    "$IMAGE_NAME"

if [[ "$DIRECT_BRIDGE" == "true" ]]; then
    APP_CONTAINER_IP=""
    for i in $(seq 1 20); do
        APP_CONTAINER_IP="$($ENGINE inspect "$CONTAINER_NAME" \
            --format '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}')"
        if [[ -n "$APP_CONTAINER_IP" ]]; then
            break
        fi
        sleep 0.5
    done
    if [[ -z "$APP_CONTAINER_IP" ]]; then
        echo "ERROR: failed to determine app container IP."
        exit 1
    fi
    APP_URL="http://$APP_CONTAINER_IP:8080"
    echo "App container IP: $APP_CONTAINER_IP."
elif [[ "$APP_PORT" == "0" ]]; then
    APP_PORT="$($ENGINE port "$CONTAINER_NAME" 8080/tcp | awk -F: 'NR == 1 { print $NF }')"
    if [[ -z "$APP_PORT" ]]; then
        echo "ERROR: failed to determine mapped app port."
        exit 1
    fi
    echo "App mapped to host port $APP_PORT."
    APP_URL="http://$APP_HOST:$APP_PORT"
else
    APP_URL="http://$APP_HOST:$APP_PORT"
fi

# --- Step 5: Wait for health check ---
echo "Waiting for app to be ready..."
for i in $(seq 1 120); do
    if curl -sf "$APP_URL/api/version" >/dev/null 2>&1; then
        echo "App is ready at $APP_URL."
        break
    fi
    if [[ $i -eq 120 ]]; then
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
read -r -a PYTEST_ARGS_ARRAY <<< "${PYTEST_ARGS:-tests/e2e/ -v}"
APP_URL="$APP_URL" \
CONTAINER="$CONTAINER_NAME" \
CONTAINER_ENGINE="$ENGINE" \
MOCK_PORT="$MOCK_PORT" \
MOCK_CONTROL_URL="$MOCK_CONTROL_URL" \
    python3 -m pytest "${PYTEST_ARGS_ARRAY[@]}" || RESULT=$?

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
