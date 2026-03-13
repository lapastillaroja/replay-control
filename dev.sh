#!/bin/bash
set -euo pipefail

# ── Replay Control Dev Script ────────────────────────────────────────────────
#
# Primary script for day-to-day development iteration.
# Supports local development with auto-reload AND fast Pi deployment.
#
# Usage:
#   ./dev.sh [--storage-path /path/to/roms] [--port 8091]
#   ./dev.sh --pi [IP]
#   ./dev.sh --pi [IP] --watch
#   ./dev.sh --pi [IP] --deploy-only
#
# Local mode (default):
#   Builds WASM (wasm-dev) + SSR (dev) and runs with cargo-watch auto-reload.
#
# Pi mode (--pi):
#   Cross-compiles for aarch64 using dev profiles (fast!), deploys to Pi.
#   --watch: auto-rebuild + redeploy on file changes.
#   --deploy-only: skip build, just deploy existing artifacts.

# ── Constants ────────────────────────────────────────────────────────────────

CRATE="replay-control-app"
OUT_DIR="target/site"
PKG_DIR="$OUT_DIR/pkg"
PORT="${PORT:-8091}"
TARGET_TRIPLE="aarch64-unknown-linux-gnu"

PI_USER="root"
PI_PASSWORD="replayos"
PI_DEFAULT_IP="<PI_IP>"
PI_INSTALL_DIR="/usr/local/bin"
PI_SITE_DIR="/usr/local/share/replay/site"
PI_SERVICE="replay-control"

SSH_OPTS="-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR -o ConnectTimeout=10"

# ── Colors ───────────────────────────────────────────────────────────────────

if [[ -t 1 ]] && [[ -n "${TERM:-}" ]] && command -v tput &>/dev/null; then
    RED=$(tput setaf 1)
    GREEN=$(tput setaf 2)
    YELLOW=$(tput setaf 3)
    BLUE=$(tput setaf 4)
    CYAN=$(tput setaf 6)
    BOLD=$(tput bold)
    DIM=$(tput dim)
    RESET=$(tput sgr0)
else
    RED="" GREEN="" YELLOW="" BLUE="" CYAN="" BOLD="" DIM="" RESET=""
fi

# ── Output helpers ───────────────────────────────────────────────────────────

phase()   { echo ""; echo "${BOLD}${BLUE}==> $*${RESET}"; }
info()    { echo "${CYAN}    $*${RESET}"; }
success() { echo "${GREEN}    ok${RESET} $*"; }
warn()    { echo "${YELLOW}    !!${RESET} $*"; }
error()   { echo "${RED}    !!${RESET} $*" >&2; }
fatal()   { error "$@"; exit 1; }

# ── Globals ──────────────────────────────────────────────────────────────────

MODE="local"        # local or pi
PI_IP=""
PI_WATCH=false
DEPLOY_ONLY=false
SERVER_ARGS=""

# SSH ControlMaster socket for connection reuse
SSH_CONTROL_DIR=""
SSH_CONTROL_SOCK=""
ASKPASS_FILE=""

# ── Argument parsing ─────────────────────────────────────────────────────────

parse_args() {
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --pi)
                MODE="pi"
                shift
                # Optional IP argument (next arg if it doesn't start with --)
                if [[ $# -gt 0 ]] && [[ "$1" != --* ]]; then
                    PI_IP="$1"
                    shift
                fi
                ;;
            --watch)
                PI_WATCH=true
                shift
                ;;
            --deploy-only)
                DEPLOY_ONLY=true
                shift
                ;;
            --port)
                [[ $# -lt 2 ]] && fatal "Missing value after --port"
                PORT="$2"
                shift 2
                ;;
            *)
                SERVER_ARGS="$SERVER_ARGS $1"
                shift
                ;;
        esac
    done

    # Validate flag combinations
    if [[ "$MODE" != "pi" ]]; then
        $DEPLOY_ONLY && fatal "--deploy-only requires --pi"
        $PI_WATCH && fatal "--watch requires --pi"
    fi
    $DEPLOY_ONLY && $PI_WATCH && fatal "--deploy-only and --watch are mutually exclusive"

    # Default Pi IP
    if [[ "$MODE" == "pi" ]] && [[ -z "$PI_IP" ]]; then
        PI_IP="$PI_DEFAULT_IP"
    fi
}

# ── Timing helper ────────────────────────────────────────────────────────────

_timer_start=""

timer_start() {
    _timer_start=$(date +%s)
}

timer_elapsed() {
    local end now elapsed
    now=$(date +%s)
    elapsed=$(( now - _timer_start ))
    if (( elapsed >= 60 )); then
        echo "$(( elapsed / 60 ))m$(( elapsed % 60 ))s"
    else
        echo "${elapsed}s"
    fi
}

# ── File size helper ─────────────────────────────────────────────────────────

human_size() {
    local bytes="$1"
    if (( bytes >= 1048576 )); then
        echo "$(( bytes / 1048576 )).$((( bytes % 1048576) * 10 / 1048576 ))M"
    elif (( bytes >= 1024 )); then
        echo "$(( bytes / 1024 ))K"
    else
        echo "${bytes}B"
    fi
}

# ── Build functions ──────────────────────────────────────────────────────────

build_wasm() {
    phase "Building WASM (wasm-dev)"
    cargo build -p "$CRATE" --lib \
        --target wasm32-unknown-unknown \
        --profile wasm-dev \
        --features hydrate \
        --no-default-features

    info "Running wasm-bindgen..."
    mkdir -p "$PKG_DIR"
    wasm-bindgen \
        "target/wasm32-unknown-unknown/wasm-dev/${CRATE//-/_}.wasm" \
        --out-dir "$PKG_DIR" \
        --out-name "${CRATE//-/_}" \
        --target web \
        --no-typescript

    local wasm_file="$PKG_DIR/${CRATE//-/_}_bg.wasm"
    # Remove stale pre-compressed file — the server uses .precompressed_gzip()
    # so a leftover .gz from build.sh would be served instead of the fresh .wasm.
    rm -f "${wasm_file}.gz"
    if [[ -f "$wasm_file" ]]; then
        local size
        size=$(stat -c%s "$wasm_file" 2>/dev/null || echo 0)
        success "WASM: $(human_size "$size")"
    fi
}

build_ssr_local() {
    phase "Building server (dev)"
    cargo build -p "$CRATE" --bin "$CRATE" \
        --features ssr \
        --no-default-features

    local bin="target/debug/$CRATE"
    if [[ -f "$bin" ]]; then
        local size
        size=$(stat -c%s "$bin" 2>/dev/null || echo 0)
        success "Binary: $(human_size "$size")"
    fi
}

build_ssr_aarch64() {
    phase "Building server (dev, aarch64)"
    setup_aarch64_sysroot

    cargo build -p "$CRATE" --bin "$CRATE" \
        --target "$TARGET_TRIPLE" \
        --features ssr \
        --no-default-features

    local bin="target/$TARGET_TRIPLE/debug/$CRATE"
    if [[ -f "$bin" ]]; then
        local size
        size=$(stat -c%s "$bin" 2>/dev/null || echo 0)
        success "Binary: $(human_size "$size") (aarch64)"
    fi
}

copy_assets() {
    mkdir -p "$OUT_DIR"
    cat replay-control-app/style/_*.css > "$OUT_DIR/style.css"
    cp -r "replay-control-app/static/icons" "$OUT_DIR/icons" 2>/dev/null || true
}

# ── aarch64 cross-compilation setup ─────────────────────────────────────────

setup_aarch64_sysroot() {
    # Skip if CFLAGS already set by the user
    if [[ -n "${CFLAGS_aarch64_unknown_linux_gnu:-}" ]]; then
        return
    fi

    local sysroot="/tmp/aarch64-sysroot"
    if [[ -f "$sysroot/usr/include/stdio.h" ]]; then
        export CFLAGS_aarch64_unknown_linux_gnu="--sysroot=$sysroot/usr -I$sysroot/usr/include"
        return
    fi

    info "Setting up aarch64 sysroot (first time only)..."
    mkdir -p /tmp/aarch64-rpms
    dnf download --forcearch=aarch64 --destdir=/tmp/aarch64-rpms glibc-devel kernel-headers 2>/dev/null
    mkdir -p "$sysroot"
    for rpm in /tmp/aarch64-rpms/*.rpm; do
        rpm2cpio "$rpm" | (cd "$sysroot" && cpio -idm 2>/dev/null)
    done

    if [[ -f "$sysroot/usr/include/stdio.h" ]]; then
        export CFLAGS_aarch64_unknown_linux_gnu="--sysroot=$sysroot/usr -I$sysroot/usr/include"
        success "aarch64 sysroot ready"
    else
        fatal "Could not set up aarch64 sysroot. Install glibc-devel.aarch64 or set CFLAGS_aarch64_unknown_linux_gnu manually."
    fi
}

# ── SSH helpers ──────────────────────────────────────────────────────────────

setup_ssh() {
    # SSH askpass for password authentication
    ASKPASS_FILE="$(mktemp)"
    printf '#!/bin/sh\necho "%s"\n' "$PI_PASSWORD" > "$ASKPASS_FILE"
    chmod +x "$ASKPASS_FILE"
    export SSH_ASKPASS="$ASKPASS_FILE"
    export SSH_ASKPASS_REQUIRE="force"
    export DISPLAY=

    # ControlMaster for SSH connection reuse
    SSH_CONTROL_DIR="$(mktemp -d)"
    SSH_CONTROL_SOCK="$SSH_CONTROL_DIR/cm-%r@%h:%p"
}

teardown_ssh() {
    # Close the ControlMaster connection
    if [[ -n "${SSH_CONTROL_SOCK:-}" ]]; then
        # shellcheck disable=SC2086
        ssh $SSH_OPTS \
            -o "ControlPath=$SSH_CONTROL_SOCK" \
            -O exit "${PI_USER}@${PI_IP}" 2>/dev/null || true
    fi
    [[ -n "${SSH_CONTROL_DIR:-}" ]] && rm -rf "$SSH_CONTROL_DIR"
    [[ -n "${ASKPASS_FILE:-}" ]] && rm -f "$ASKPASS_FILE"
}

run_ssh() {
    # shellcheck disable=SC2086
    ssh $SSH_OPTS \
        -o "ControlMaster=auto" \
        -o "ControlPath=$SSH_CONTROL_SOCK" \
        -o "ControlPersist=300" \
        "${PI_USER}@${PI_IP}" "$@"
}

run_rsync() {
    # shellcheck disable=SC2086
    rsync -e "ssh $SSH_OPTS -o ControlMaster=auto -o ControlPath=$SSH_CONTROL_SOCK -o ControlPersist=300" "$@"
}

check_pi_connectivity() {
    phase "Connecting to Pi at ${PI_IP}"
    if ! run_ssh true 2>/dev/null; then
        fatal "Cannot SSH to ${PI_USER}@${PI_IP}. Is the Pi powered on and on the network?"
    fi
    success "SSH connection established (ControlMaster active)"
}

# ── Pi deployment ────────────────────────────────────────────────────────────

deploy_to_pi() {
    local bin_path="target/$TARGET_TRIPLE/debug/$CRATE"

    if [[ ! -f "$bin_path" ]]; then
        fatal "Binary not found: $bin_path — build first or remove --deploy-only"
    fi
    if [[ ! -d "$OUT_DIR" ]]; then
        fatal "Site assets not found: $OUT_DIR — build first or remove --deploy-only"
    fi

    local bin_size
    bin_size=$(stat -c%s "$bin_path" 2>/dev/null || echo 0)

    phase "Deploying to Pi (${PI_IP})"

    # Stop service before overwriting binary
    info "Stopping service..."
    run_ssh "systemctl stop $PI_SERVICE 2>/dev/null || true"

    # Transfer binary
    info "Syncing binary ($(human_size "$bin_size"))..."
    run_rsync --compress "$bin_path" "${PI_USER}@${PI_IP}:${PI_INSTALL_DIR}/replay-control-app"
    run_ssh "chmod +x ${PI_INSTALL_DIR}/replay-control-app"

    # Transfer site assets (rsync only changed files)
    info "Syncing site assets..."
    run_rsync -r --compress --delete \
        "$OUT_DIR/" "${PI_USER}@${PI_IP}:${PI_SITE_DIR}/"

    # Restart service
    info "Starting service..."
    run_ssh "systemctl start $PI_SERVICE"

    # Quick health check
    if run_ssh "systemctl is-active $PI_SERVICE" &>/dev/null; then
        success "Service is running"
    else
        warn "Service may not have started. Check: ssh ${PI_USER}@${PI_IP} journalctl -u $PI_SERVICE -n 20"
    fi

    echo ""
    echo "    ${GREEN}${BOLD}http://${PI_IP}:8080${RESET}"
    echo ""
}

# ── Local mode ───────────────────────────────────────────────────────────────

run_local() {
    timer_start
    phase "Initial build (local)"
    build_wasm
    build_ssr_local
    copy_assets
    echo ""
    info "Build completed in ${BOLD}$(timer_elapsed)${RESET}"
    echo ""

    phase "Starting cargo-watch on port ${PORT}"
    info "Watching: replay-control-app/src, replay-control-core/src, replay-control-app/style"
    info "Press Ctrl+C to stop."
    echo ""

    exec cargo watch \
        -w replay-control-app/src \
        -w replay-control-core/src \
        -w replay-control-app/style \
        -s "$(cat <<INNER
set -e
BUILD_START=\$(date +%s)
cargo build -p $CRATE --lib --target wasm32-unknown-unknown --profile wasm-dev --features hydrate --no-default-features
wasm-bindgen target/wasm32-unknown-unknown/wasm-dev/${CRATE//-/_}.wasm --out-dir $PKG_DIR --out-name ${CRATE//-/_} --target web --no-typescript
rm -f $PKG_DIR/${CRATE//-/_}_bg.wasm.gz
cat replay-control-app/style/_*.css > $OUT_DIR/style.css
cargo build -p $CRATE --bin $CRATE --features ssr --no-default-features
BUILD_END=\$(date +%s)
echo ""
echo "    Rebuilt in \$(( BUILD_END - BUILD_START ))s"
echo ""
cargo run -p $CRATE --features ssr --no-default-features -- --port $PORT $SERVER_ARGS
INNER
)"
}

# ── Pi mode ──────────────────────────────────────────────────────────────────

run_pi() {
    trap teardown_ssh EXIT
    setup_ssh
    check_pi_connectivity

    if $DEPLOY_ONLY; then
        deploy_to_pi
        return
    fi

    # Build + deploy
    pi_build_and_deploy

    # Watch mode: rebuild + redeploy on changes
    if $PI_WATCH; then
        phase "Watching for changes (Pi auto-deploy)"
        info "Watching: replay-control-app/src, replay-control-core/src, replay-control-app/style"
        info "Press Ctrl+C to stop."
        echo ""

        # The cargo-watch subprocess inherits our environment (SSH_ASKPASS etc.)
        # but we need to pass the resolved values for SSH control socket and CFLAGS
        # since those are set up by functions that ran in this process.
        local resolved_cflags="${CFLAGS_aarch64_unknown_linux_gnu:-}"

        cargo watch \
            -w replay-control-app/src \
            -w replay-control-core/src \
            -w replay-control-app/style \
            -s "$(cat <<INNER
set -e
BUILD_START=\$(date +%s)

echo ""
echo "${BOLD}${BLUE}==> Rebuilding WASM (wasm-dev)${RESET}"
cargo build -p $CRATE --lib --target wasm32-unknown-unknown --profile wasm-dev --features hydrate --no-default-features
wasm-bindgen target/wasm32-unknown-unknown/wasm-dev/${CRATE//-/_}.wasm --out-dir $PKG_DIR --out-name ${CRATE//-/_} --target web --no-typescript
cat replay-control-app/style/_*.css > $OUT_DIR/style.css
cp -r replay-control-app/static/icons $OUT_DIR/icons 2>/dev/null || true

echo "${BOLD}${BLUE}==> Rebuilding server (dev, aarch64)${RESET}"
export CFLAGS_aarch64_unknown_linux_gnu="$resolved_cflags"
cargo build -p $CRATE --bin $CRATE --target $TARGET_TRIPLE --features ssr --no-default-features

BUILD_END=\$(date +%s)
echo ""
echo "${CYAN}    Rebuilt in \$(( BUILD_END - BUILD_START ))s${RESET}"

echo "${BOLD}${BLUE}==> Deploying to Pi ($PI_IP)${RESET}"
SSH_CMD="ssh $SSH_OPTS -o ControlMaster=auto -o ControlPath=$SSH_CONTROL_SOCK -o ControlPersist=300"

\$SSH_CMD ${PI_USER}@${PI_IP} "systemctl stop $PI_SERVICE 2>/dev/null || true"
rsync -e "\$SSH_CMD" --compress target/$TARGET_TRIPLE/debug/$CRATE ${PI_USER}@${PI_IP}:${PI_INSTALL_DIR}/replay-control-app
\$SSH_CMD ${PI_USER}@${PI_IP} "chmod +x ${PI_INSTALL_DIR}/replay-control-app"
rsync -e "\$SSH_CMD" -r --compress --delete $OUT_DIR/ ${PI_USER}@${PI_IP}:${PI_SITE_DIR}/
\$SSH_CMD ${PI_USER}@${PI_IP} "systemctl start $PI_SERVICE"

echo ""
echo "${GREEN}    Deployed! ${BOLD}http://${PI_IP}:8080${RESET}"
echo ""
INNER
)"
    fi
}

pi_build_and_deploy() {
    timer_start

    # WASM is architecture-independent, same build for both local and Pi
    build_wasm
    build_ssr_aarch64
    copy_assets

    echo ""
    info "Build completed in ${BOLD}$(timer_elapsed)${RESET}"

    deploy_to_pi
}

# ── Main ─────────────────────────────────────────────────────────────────────

main() {
    parse_args "$@"

    case "$MODE" in
        local)
            run_local
            ;;
        pi)
            run_pi
            ;;
    esac
}

main "$@"
