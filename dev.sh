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
#
#   ./dev.sh --clean                  # clear cargo + sccache caches before build
#
# Local mode (default):
#   Builds WASM (wasm-dev) + SSR (dev) and runs with cargo-watch auto-reload.
#
# Pi mode (--pi):
#   Cross-compiles for aarch64 using dev profiles (fast!), deploys to Pi.
#

# ── Constants ────────────────────────────────────────────────────────────────

CRATE="replay-control-app"
CRATE_SNAKE="${CRATE//-/_}"
TARGET_DIR="${CARGO_TARGET_DIR:-target}"
OUT_DIR="$TARGET_DIR/site"
PKG_DIR="$OUT_DIR/pkg"
PORT="${PORT:-8091}"
TARGET_TRIPLE="aarch64-unknown-linux-gnu"

PI_USER="root"
PI_PASSWORD="${PI_PASS:-replayos}"
PI_DEFAULT_IP="replay.local"
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
CLEAN=false

SERVER_ARGS=""

# Enable sccache for faster incremental dev builds (if available).
if command -v sccache &>/dev/null; then
    export RUSTC_WRAPPER=sccache
fi


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
            --clean)
                CLEAN=true
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

ssr_features() {
    echo "ssr"
}

build_wasm() {
    phase "Building WASM (wasm-dev)"
    cargo build -p "$CRATE" --lib \
        --target wasm32-unknown-unknown \
        --profile wasm-dev \
        --features hydrate \
        --no-default-features

    info "Running wasm-bindgen..."
    mkdir -p "$PKG_DIR"
    # Drop previously hashed assets so old hashes don't accumulate locally.
    rm -f "$PKG_DIR/${CRATE_SNAKE}".*.wasm "$PKG_DIR/${CRATE_SNAKE}".*.wasm.gz "$PKG_DIR/${CRATE_SNAKE}".*.js
    wasm-bindgen \
        "$TARGET_DIR/wasm32-unknown-unknown/wasm-dev/${CRATE_SNAKE}.wasm" \
        --out-dir "$PKG_DIR" \
        --out-name "${CRATE_SNAKE}" \
        --target web \
        --no-typescript

    info "Hashing assets..."
    local wasm_file="$PKG_DIR/${CRATE_SNAKE}_bg.wasm"
    local js_file="$PKG_DIR/${CRATE_SNAKE}.js"
    local wasm_hash hashed_wasm js_hash hashed_js
    wasm_hash=$(sha256sum "$wasm_file" | cut -c1-16)
    hashed_wasm="$PKG_DIR/${CRATE_SNAKE}.${wasm_hash}.wasm"
    mv "$wasm_file" "$hashed_wasm"
    sed -i "s|${CRATE_SNAKE}_bg\.wasm|${CRATE_SNAKE}.${wasm_hash}.wasm|g" "$js_file"
    js_hash=$(sha256sum "$js_file" | cut -c1-16)
    hashed_js="$PKG_DIR/${CRATE_SNAKE}.${js_hash}.js"
    mv "$js_file" "$hashed_js"
    printf 'js: %s\nwasm: %s\n' "$js_hash" "$wasm_hash" > "$OUT_DIR/hash.txt"

    if [[ -f "$hashed_wasm" ]]; then
        local size
        size=$(stat -c%s "$hashed_wasm" 2>/dev/null || echo 0)
        success "WASM: $(human_size "$size")  hash: ${wasm_hash}"
    fi
}

build_ssr_local() {
    local features
    features=$(ssr_features)
    phase "Building server (dev)"
    cargo build -p "$CRATE" --bin "$CRATE" \
        --features "$features" \
        --no-default-features

    local bin="$TARGET_DIR/debug/$CRATE"
    if [[ -f "$bin" ]]; then
        local size
        size=$(stat -c%s "$bin" 2>/dev/null || echo 0)
        success "Binary: $(human_size "$size")"
    fi
}

build_ssr_aarch64() {
    local features
    features=$(ssr_features)
    phase "Building server (dev, aarch64)"
    check_aarch64_sysroot

    cargo build -p "$CRATE" --bin "$CRATE" \
        --target "$TARGET_TRIPLE" \
        --features "$features" \
        --no-default-features

    local bin="$TARGET_DIR/$TARGET_TRIPLE/debug/$CRATE"
    if [[ -f "$bin" ]]; then
        local size
        size=$(stat -c%s "$bin" 2>/dev/null || echo 0)
        success "Binary: $(human_size "$size") (aarch64)"
    fi
}

copy_assets() {
    mkdir -p "$OUT_DIR"
    cat replay-control-app/style/_*.css > "$OUT_DIR/style.css"
    rm -rf "$OUT_DIR/icons" "$OUT_DIR/branding"
    cp -r "replay-control-app/static/icons" "$OUT_DIR/icons" 2>/dev/null || true
    cp -r "replay-control-app/static/branding" "$OUT_DIR/branding" 2>/dev/null || true
}

# ── aarch64 cross-compilation setup ─────────────────────────────────────────

check_aarch64_sysroot() {
    AARCH64_SYSROOT="${AARCH64_SYSROOT:-}"
    AARCH64_INCLUDE=""
    if [[ -z "$AARCH64_SYSROOT" ]]; then
        # Auto-detect across layouts:
        #   Fedora:            /usr/aarch64-linux-gnu/sys-root/usr/include/stdio.h
        #   Ubuntu/older Deb:  /usr/aarch64-linux-gnu/usr/include/stdio.h
        #   Debian trixie+:    /usr/aarch64-linux-gnu/include/stdio.h     (no /usr/)
        for _sysroot in "/usr/aarch64-linux-gnu/sys-root" "/usr/aarch64-linux-gnu"; do
            for _inc in "$_sysroot/usr/include" "$_sysroot/include"; do
                if [[ -f "$_inc/stdio.h" ]]; then
                    AARCH64_SYSROOT="$_sysroot"
                    AARCH64_INCLUDE="$_inc"
                    break 2
                fi
            done
        done
    elif [[ -f "$AARCH64_SYSROOT/usr/include/stdio.h" ]]; then
        AARCH64_INCLUDE="$AARCH64_SYSROOT/usr/include"
    elif [[ -f "$AARCH64_SYSROOT/include/stdio.h" ]]; then
        AARCH64_INCLUDE="$AARCH64_SYSROOT/include"
    fi
    if [[ -z "$AARCH64_SYSROOT" || -z "$AARCH64_INCLUDE" ]]; then
        echo ""
        echo "  aarch64 cross-compile sysroot not found."
        echo ""
        echo "  Searched: /usr/aarch64-linux-gnu/sys-root (Fedora)"
        echo "           /usr/aarch64-linux-gnu           (Debian/Ubuntu)"
        echo ""
        echo "  Set AARCH64_SYSROOT to override. See CONTRIBUTING.md for cross-compilation setup."
        echo ""
        fatal "aarch64 sysroot missing."
    fi
    info "Sysroot: $AARCH64_SYSROOT (include: $AARCH64_INCLUDE)"
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

# Write the systemd unit + env file on the Pi when missing. Mirrors what
# install.sh emits via systemd_service_content + env_file_content; keep them
# in sync. No-op when the unit already exists, so it stays cheap on every run.
bootstrap_pi_if_needed() {
    if run_ssh "systemctl cat $PI_SERVICE >/dev/null 2>&1"; then
        return 0
    fi
    info "Service unit missing on Pi — writing systemd unit + env file..."
    run_ssh "bash -s" <<'BOOTSTRAP'
set -euo pipefail

mkdir -p /etc/systemd/system /etc/default

cat > /etc/systemd/system/replay-control.service <<'UNIT'
[Unit]
Description=Replay Control
After=network.target
After=media-sd.mount media-usb.mount

[Service]
Type=simple
EnvironmentFile=-/etc/default/replay-control
ExecStart=/usr/local/bin/replay-control-app \
    --port ${REPLAY_PORT} \
    --site-root ${REPLAY_SITE_ROOT}
Restart=on-failure
RestartSec=5
StandardOutput=append:/var/log/replay-control.log
StandardError=append:/var/log/replay-control.log
SyslogIdentifier=replay-control

[Install]
WantedBy=multi-user.target
UNIT

if [ ! -f /etc/default/replay-control ]; then
    cat > /etc/default/replay-control <<'ENV'
# Port for the web UI
REPLAY_PORT=8080

# Path to static site assets
REPLAY_SITE_ROOT=/usr/local/share/replay/site

# Log level (trace, debug, info, warn, error)
RUST_LOG=replay_control_app=info,replay_control_core=info
ENV
fi

systemctl daemon-reload
systemctl enable replay-control >/dev/null 2>&1 || true
BOOTSTRAP
    success "Bootstrapped systemd unit"
}

deploy_to_pi() {
    local bin_path="$TARGET_DIR/$TARGET_TRIPLE/debug/$CRATE"

    if [[ ! -f "$bin_path" ]]; then
        fatal "Binary not found: $bin_path"
    fi
    if [[ ! -d "$OUT_DIR" ]]; then
        fatal "Site assets not found: $OUT_DIR"
    fi

    local bin_size
    bin_size=$(stat -c%s "$bin_path" 2>/dev/null || echo 0)

    phase "Deploying to Pi (${PI_IP})"

    # Bootstrap the service unit + env file if missing (e.g. fresh OS image
    # or after `install.sh --purge`). Keep this in sync with install.sh's
    # systemd_service_content / env_file_content — drift here means dev.sh
    # would deploy onto a Pi that disagrees with what install.sh produces.
    bootstrap_pi_if_needed

    # Stop service before overwriting binary
    info "Stopping service..."
    run_ssh "systemctl stop $PI_SERVICE 2>/dev/null || true"
    run_ssh "rm -rf /var/tmp/replay-control-update /var/tmp/replay-control-update.lock /var/tmp/replay-control-do-update.sh 2>/dev/null || true"

    # Ensure install dirs exist — rsync only auto-creates the last component
    # of the destination path, so a missing $PI_SITE_DIR parent (e.g. after
    # `install.sh --purge`) makes the site sync below fail.
    run_ssh "mkdir -p $PI_INSTALL_DIR $PI_SITE_DIR"

    # Transfer binary
    info "Syncing binary ($(human_size "$bin_size"))..."
    run_rsync --compress "$bin_path" "${PI_USER}@${PI_IP}:${PI_INSTALL_DIR}/replay-control-app"
    run_ssh "chmod +x ${PI_INSTALL_DIR}/replay-control-app"

    # Transfer catalog. Hard fail if missing — shipping with an absent (or
    # zero-byte) catalog leaves SQLite to silently open an empty DB and every
    # catalog query then fails with "no such table: arcade_games". Build the
    # real catalog first rather than continuing with a broken deploy.
    if [[ ! -s catalog.sqlite ]]; then
        fatal "catalog.sqlite not found or empty — run: cargo run -p build-catalog -- --output catalog.sqlite"
    fi
    info "Syncing catalog..."
    run_rsync --compress catalog.sqlite "${PI_USER}@${PI_IP}:${PI_INSTALL_DIR}/catalog.sqlite"

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

ensure_catalog_local() {
    # Build the real catalog (not the stub fixture). Stub data only has a
    # handful of games; shipping it to a real Pi hides most ROMs' metadata
    # and looks like a regression. Missing data files are a hard error so
    # the caller notices instead of silently running with a broken catalog.
    if [[ ! -f catalog.sqlite ]]; then
        phase "Building game catalog (catalog.sqlite not found)"
        if ! cargo run -p build-catalog -- --output catalog.sqlite; then
            fatal "Failed to build catalog.sqlite. Run ./build.sh first, or \
check that data/ is populated (run ./scripts/download-arcade-data.sh and \
./scripts/download-metadata.sh)."
        fi
    fi
    if [[ ! -s catalog.sqlite ]]; then
        fatal "catalog.sqlite is empty or missing after build step."
    fi
}

run_local() {
    timer_start
    ensure_catalog_local
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

    local features
    features=$(ssr_features)

    exec cargo watch \
        -w replay-control-app/src \
        -w replay-control-core/src \
        -w replay-control-app/style \
        -s "$(cat <<INNER
set -e
BUILD_START=\$(date +%s)
cargo build -p $CRATE --lib --target wasm32-unknown-unknown --profile wasm-dev --features hydrate --no-default-features
rm -f $PKG_DIR/${CRATE_SNAKE}.*.wasm $PKG_DIR/${CRATE_SNAKE}.*.wasm.gz $PKG_DIR/${CRATE_SNAKE}.*.js
wasm-bindgen $TARGET_DIR/wasm32-unknown-unknown/wasm-dev/${CRATE_SNAKE}.wasm --out-dir $PKG_DIR --out-name ${CRATE_SNAKE} --target web --no-typescript
WASM_HASH=\$(sha256sum $PKG_DIR/${CRATE_SNAKE}_bg.wasm | cut -c1-16)
mv $PKG_DIR/${CRATE_SNAKE}_bg.wasm $PKG_DIR/${CRATE_SNAKE}.\${WASM_HASH}.wasm
sed -i "s|${CRATE_SNAKE}_bg\\.wasm|${CRATE_SNAKE}.\${WASM_HASH}.wasm|g" $PKG_DIR/${CRATE_SNAKE}.js
JS_HASH=\$(sha256sum $PKG_DIR/${CRATE_SNAKE}.js | cut -c1-16)
mv $PKG_DIR/${CRATE_SNAKE}.js $PKG_DIR/${CRATE_SNAKE}.\${JS_HASH}.js
printf 'js: %s\nwasm: %s\n' "\${JS_HASH}" "\${WASM_HASH}" > $OUT_DIR/hash.txt
cat replay-control-app/style/_*.css > $OUT_DIR/style.css
cargo build -p $CRATE --bin $CRATE --features $features --no-default-features
BUILD_END=\$(date +%s)
echo ""
echo "    Rebuilt in \$(( BUILD_END - BUILD_START ))s"
echo ""
cargo run -p $CRATE --features $features --no-default-features -- --port $PORT $SERVER_ARGS
INNER
)"
}

# ── Pi mode ──────────────────────────────────────────────────────────────────

run_pi() {
    trap teardown_ssh EXIT
    setup_ssh
    check_pi_connectivity

    pi_build_and_deploy
}

pi_build_and_deploy() {
    timer_start

    # Build the catalog before anything else so the Pi always receives a
    # real, non-empty DB. deploy_to_pi hard-fails if catalog.sqlite is
    # missing, but preflighting here gives a cleaner failure earlier.
    ensure_catalog_local

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

    if $CLEAN; then
        phase "Cleaning build cache"
        cargo clean 2>/dev/null
        if command -v sccache &>/dev/null; then
            sccache --stop-server 2>/dev/null || true
            rm -rf "${SCCACHE_DIR:-$HOME/.cache/sccache}" 2>/dev/null
            info "sccache cache cleared"
        fi
        success "Clean complete"
    fi

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
