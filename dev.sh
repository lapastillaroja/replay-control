#!/usr/bin/env bash
set -euo pipefail

CRATE=replay-control-app
TARGET_TRIPLE=aarch64-unknown-linux-gnu
TARGET_DIR=${CARGO_TARGET_DIR:-target}
SITE_DIR=$TARGET_DIR/site
WASM_PROFILE=${REPLAY_DEV_WASM_PROFILE:-wasm-dev-fast}
SERVER_PROFILE=${REPLAY_DEV_SERVER_PROFILE:-dev-fast}
PI_HOST=replay.local
PI_USER=root
PI_PASSWORD=${PI_PASS:-replayos}
DEV_ROOT=/var/lib/replay-control/dev
DROPIN=/etc/systemd/system/replay-control.service.d/90-replay-control-dev.conf
SSH_OPTS=(-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR -o ConnectTimeout=10)
BUILD_ONLY=false
RESET=false
CLEAN=false
SSH_SOCKET_DIR=
ASKPASS_FILE=
STAGE_LOCAL=
STAGE_REMOTE=

if command -v sccache >/dev/null 2>&1; then
    export RUSTC_WRAPPER=sccache
fi

usage() {
    cat <<'USAGE'
Usage: ./dev.sh [--pi [HOST]] [--clean]
       ./dev.sh --build-only [--clean]
       ./dev.sh --pi [HOST] --reset

Build the latest code with fast profiles and deploy it to the Pi. --build-only
builds native test artifacts without deploying. --reset removes the Pi's
development override and returns to the installed service.
USAGE
}

parse_args() {
    while (($#)); do
        case $1 in
            --pi)
                shift
                if (($#)) && [[ $1 != --* ]]; then PI_HOST=$1; shift; fi ;;
            --build-only) BUILD_ONLY=true; shift ;;
            --reset) RESET=true; shift ;;
            --clean) CLEAN=true; shift ;;
            --package) echo 'Use install.sh --package FILE for a prebuilt package.' >&2; exit 2 ;;
            --help|-h) usage; exit 0 ;;
            *) echo "Unknown option: $1" >&2; usage >&2; exit 2 ;;
        esac
    done
    if $BUILD_ONLY && $RESET; then
        echo '--build-only and --reset cannot be combined' >&2
        exit 2
    fi
    if $CLEAN && $RESET; then
        echo '--clean and --reset cannot be combined' >&2
        exit 2
    fi
}

profile_dir() {
    if [[ $1 == dev ]]; then printf 'debug'; else printf '%s' "$1"; fi
}

ensure_catalog() {
    if [[ ! -s catalog.sqlite ]]; then
        cargo run -p build-catalog -- --output catalog.sqlite
    fi
    [[ -s catalog.sqlite ]] || { echo 'catalog.sqlite is missing' >&2; exit 1; }
}

validate_catalog_for_pi() {
    local is_stub
    is_stub=$(python3 -c 'import sqlite3; print(sqlite3.connect("catalog.sqlite").execute("SELECT value FROM db_meta WHERE key=\x27is_stub\x27").fetchone()[0])') || {
        echo 'Could not validate catalog.sqlite' >&2
        exit 1
    }
    if [[ $is_stub == 1 ]]; then
        echo 'Refusing to deploy a stub catalog to the Pi' >&2
        exit 1
    fi
}

build_wasm() {
    echo "==> Building WASM ($WASM_PROFILE)"
    cargo build -p "$CRATE" --lib --target wasm32-unknown-unknown \
        --profile "$WASM_PROFILE" --features hydrate --no-default-features
    local wasm_dir
    wasm_dir=$(profile_dir "$WASM_PROFILE")
    mkdir -p "$SITE_DIR/pkg"
    rm -f "$SITE_DIR/pkg/replay_control_app."*.wasm \
        "$SITE_DIR/pkg/replay_control_app."*.wasm.gz \
        "$SITE_DIR/pkg/replay_control_app."*.js
    wasm-bindgen "$TARGET_DIR/wasm32-unknown-unknown/$wasm_dir/replay_control_app.wasm" \
        --out-dir "$SITE_DIR/pkg" --out-name replay_control_app \
        --target web --no-typescript --remove-name-section
    local wasm_file=$SITE_DIR/pkg/replay_control_app_bg.wasm
    local js_file=$SITE_DIR/pkg/replay_control_app.js
    local wasm_hash js_hash
    wasm_hash=$(sha256sum "$wasm_file" | cut -c1-16)
    mv "$wasm_file" "$SITE_DIR/pkg/replay_control_app.$wasm_hash.wasm"
    sed -i "s|replay_control_app_bg\.wasm|replay_control_app.$wasm_hash.wasm|g" "$js_file"
    js_hash=$(sha256sum "$js_file" | cut -c1-16)
    mv "$js_file" "$SITE_DIR/pkg/replay_control_app.$js_hash.js"
    printf 'js: %s\nwasm: %s\n' "$js_hash" "$wasm_hash" > "$SITE_DIR/hash.txt"
    cat replay-control-app/style/_*.css > "$SITE_DIR/style.css"
    rm -rf "$SITE_DIR/icons" "$SITE_DIR/branding"
    cp -r replay-control-app/static/icons "$SITE_DIR/icons"
    cp -r replay-control-app/static/branding "$SITE_DIR/branding"
}

check_cross_sysroot() {
    local sysroot=${AARCH64_SYSROOT:-}
    if [[ -z $sysroot ]]; then
        for candidate in /usr/aarch64-linux-gnu/sys-root /usr/aarch64-linux-gnu; do
            if [[ -f $candidate/usr/include/stdio.h || -f $candidate/include/stdio.h ]]; then
                sysroot=$candidate
                break
            fi
        done
    fi
    [[ -n $sysroot && ( -f $sysroot/usr/include/stdio.h || -f $sysroot/include/stdio.h ) ]] || {
        echo 'aarch64 cross-compilation sysroot missing; see CONTRIBUTING.md' >&2
        exit 1
    }
}

build_server() {
    if $BUILD_ONLY; then
        echo "==> Building native server ($SERVER_PROFILE)"
        cargo build -p "$CRATE" --bin "$CRATE" --profile "$SERVER_PROFILE" \
            --features ssr --no-default-features
    else
        check_cross_sysroot
        echo "==> Building arm64 server ($SERVER_PROFILE)"
        cargo build -p "$CRATE" --bin "$CRATE" --profile "$SERVER_PROFILE" \
            --target "$TARGET_TRIPLE" --features ssr --no-default-features
    fi
}

setup_ssh() {
    ASKPASS_FILE=$(mktemp)
    printf '#!/bin/sh\nprintf "%%s\\n" "$REPLAY_DEV_PI_PASS"\n' > "$ASKPASS_FILE"
    chmod 700 "$ASKPASS_FILE"
    export REPLAY_DEV_PI_PASS=$PI_PASSWORD
    export SSH_ASKPASS=$ASKPASS_FILE SSH_ASKPASS_REQUIRE=force DISPLAY=
    SSH_SOCKET_DIR=$(mktemp -d)
}

teardown_ssh() {
    if [[ -n $SSH_SOCKET_DIR ]]; then
        ssh "${SSH_OPTS[@]}" -o "ControlPath=$SSH_SOCKET_DIR/socket" \
            -O exit "$PI_USER@$PI_HOST" >/dev/null 2>&1 || true
        rm -rf "$SSH_SOCKET_DIR"
    fi
    [[ -z $ASKPASS_FILE ]] || rm -f "$ASKPASS_FILE"
    unset REPLAY_DEV_PI_PASS
}

cleanup() {
    [[ -z $STAGE_LOCAL ]] || rm -rf "$STAGE_LOCAL"
    if [[ $STAGE_REMOTE == "$DEV_ROOT/releases/.staging."* ]]; then
        remote "rm -rf '$STAGE_REMOTE'" >/dev/null 2>&1 || true
    fi
    teardown_ssh
}

remote() {
    ssh "${SSH_OPTS[@]}" -o ControlMaster=auto \
        -o "ControlPath=$SSH_SOCKET_DIR/socket" -o ControlPersist=300 \
        "$PI_USER@$PI_HOST" "$@"
}

sync_remote() {
    rsync -e "ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR -o ConnectTimeout=10 -o ControlMaster=auto -o ControlPath=$SSH_SOCKET_DIR/socket -o ControlPersist=300" "$@"
}

reset_overlay() {
    remote bash -s -- "$DROPIN" "$DEV_ROOT" <<'REMOTE'
set -euo pipefail
dropin=$1
root=$2
mkdir -p "$root"
exec 9>"$root/deploy.lock"
flock -n 9 || { echo 'Another development deployment is running' >&2; exit 1; }
if systemctl is-active --quiet replay-control-package-update.service; then
    echo 'A package update is running; retry after it finishes' >&2
    exit 1
fi
if [[ -e $dropin ]]; then
    saved=$(mktemp -d)
    trap 'rm -rf "$saved"' EXIT
    [[ ! -e $dropin ]] || cp -a "$dropin" "$saved/dev.conf"
    rm -f "$dropin"
    changed=true
    if ! systemctl daemon-reload || ! systemctl restart replay-control.service; then
        changed=false
    fi
    port=$(sed -n 's/^REPLAY_HTTPS_PORT=//p' /etc/default/replay-control 2>/dev/null | tail -1)
    [[ $port =~ ^[0-9]+$ ]] || port=8443
    http_port=$(sed -n 's/^REPLAY_PORT=//p' /etc/default/replay-control 2>/dev/null | tail -1)
    [[ $http_port =~ ^[0-9]+$ ]] || http_port=8080
    healthy=false
    if $changed; then
        for _ in {1..30}; do
            pid=$(systemctl show -p MainPID --value replay-control.service)
            if [[ $pid =~ ^[1-9][0-9]*$ ]] &&
               [[ $(readlink -f "/proc/$pid/exe" 2>/dev/null) != "$root/releases/"* ]] &&
               { curl -kfsS --max-time 3 "https://127.0.0.1:$port/api/version" >/dev/null 2>&1 ||
                 curl -fsS --max-time 3 "http://127.0.0.1:$http_port/api/version" >/dev/null 2>&1; }; then
                healthy=true
                break
            fi
            sleep 2
        done
    fi
    if ! $healthy; then
        [[ ! -e $saved/dev.conf ]] || cp -a "$saved/dev.conf" "$dropin"
        systemctl daemon-reload
        systemctl restart replay-control.service || true
        echo 'Installed service failed; development override restored' >&2
        exit 1
    fi
fi
systemctl is-active --quiet replay-control.service
REMOTE
    echo 'Pi is running its installed Replay Control service'
}

bootstrap_if_needed() {
    local binary=$1
    if remote 'systemctl cat replay-control.service >/dev/null 2>&1'; then
        return
    fi
    remote bash -s <<'REMOTE'
set -e
if [ ! -e /etc/replay-control/settings.cfg ] &&
   [ ! -e /usr/local/bin/replay-control-app ] &&
   [ ! -e /usr/bin/replay-control-app ]; then
    for storage in /media/*; do
        if [ -e "$storage/.replay-control/settings.cfg" ]; then
            exit 0
        fi
    done
    mkdir -p /etc/replay-control
    printf 'analytics = "false"\n' > /etc/replay-control/settings.cfg
fi
REMOTE
    echo '==> Installing Replay Control archive service on a clean Pi'
    REPLAY_LOCAL_BINARY=$(realpath "$binary") PI_PASS=$PI_PASSWORD \
        "$(dirname "$0")/install.sh" --local "$(pwd)" --ip "$PI_HOST"
}

deploy() {
    local binary=$1
    [[ -x $binary && -s catalog.sqlite && -d $SITE_DIR/pkg ]] || {
        echo 'Built binary, catalog, or site is missing' >&2
        exit 1
    }
    "$(dirname "$0")/scripts/verify-package-server-routes.sh" "$binary" "$SITE_DIR"
    if remote 'systemctl is-active --quiet replay-control-package-update.service'; then
        echo 'A package update is running on the Pi; retry after it finishes' >&2
        exit 1
    fi
    bootstrap_if_needed "$binary"
    remote "mkdir -p '$DEV_ROOT/releases'"
    local remote_stage
    remote_stage=$(remote "mktemp -d '$DEV_ROOT/releases/.staging.XXXXXX'")
    STAGE_REMOTE=$remote_stage
    local local_stage
    local_stage=$(mktemp -d)
    STAGE_LOCAL=$local_stage
    ln -s "$(realpath "$binary")" "$local_stage/replay-control-app"
    ln -s "$(realpath catalog.sqlite)" "$local_stage/catalog.sqlite"
    ln -s "$(realpath "$SITE_DIR")" "$local_stage/site"
    ln -s "$(realpath packaging/start-service)" "$local_stage/start-service"
    echo "==> Syncing current development build to $PI_HOST"
    local previous_root
    previous_root=$(remote "if [ -L '$DEV_ROOT/current' ]; then readlink -f '$DEV_ROOT/current'; fi")
    local link_args=()
    [[ -z $previous_root ]] || link_args=(--link-dest="$previous_root")
    sync_remote -aL "${link_args[@]}" "$local_stage/" \
        "$PI_USER@$PI_HOST:$remote_stage/"
    remote bash -s -- "$remote_stage" "$DEV_ROOT" "$DROPIN" <<'REMOTE'
set -euo pipefail
stage=$1
root=$2
dropin=$3
exec 9>"$root/deploy.lock"
flock -n 9 || { echo 'Another development deployment is running' >&2; exit 1; }
if systemctl is-active --quiet replay-control-package-update.service; then
    echo 'A package update is running; retry after it finishes' >&2
    exit 1
fi
new="$root/releases/$(date +%s)-$$"
previous=''
if [[ -L $root/current ]]; then previous=$(readlink -f "$root/current"); fi
expected=$(printf '[Service]\nEnvironment=REPLAY_CONTROL_DEV_ROOT=%s/current\nExecStart=\nExecStart=%s/current/start-service\n' "$root" "$root")
if [[ -e $dropin ]] && [[ $(cat "$dropin") != "$expected" ]]; then
    echo "Unexpected systemd development override: $dropin" >&2
    exit 1
fi
had_dropin=false
[[ ! -e $dropin ]] || had_dropin=true
changed=false
rollback() {
    if $changed; then
        if [[ -n $previous ]]; then
            link="$root/.rollback.$$"
            ln -s "$previous" "$link"
            mv -Tf "$link" "$root/current"
        else
            rm -f "$root/current"
        fi
        if ! $had_dropin; then rm -f "$dropin"; fi
        systemctl daemon-reload || true
        systemctl restart replay-control.service || true
    fi
    rm -rf "$new"
    echo 'Development deployment failed; previous service restored' >&2
}
on_exit() {
    status=$?
    trap - EXIT
    if (( status != 0 )); then rollback; fi
    exit "$status"
}
trap on_exit EXIT
mv "$stage" "$new"
chmod 755 "$new/start-service" "$new/replay-control-app"
mkdir -p "$(dirname "$dropin")"
changed=true
if [[ ! -e $dropin ]]; then printf '%s\n' "$expected" > "$dropin"; fi
link="$root/.current.$$"
ln -s "$new" "$link"
mv -Tf "$link" "$root/current"
if ! systemctl daemon-reload || ! systemctl restart replay-control.service; then
    exit 1
fi
port=$(sed -n 's/^REPLAY_HTTPS_PORT=//p' /etc/default/replay-control 2>/dev/null | tail -1)
[[ $port =~ ^[0-9]+$ ]] || port=8443
http_port=$(sed -n 's/^REPLAY_PORT=//p' /etc/default/replay-control 2>/dev/null | tail -1)
[[ $http_port =~ ^[0-9]+$ ]] || http_port=8080
healthy=false
for _ in {1..30}; do
    pid=$(systemctl show -p MainPID --value replay-control.service)
    if [[ $pid =~ ^[1-9][0-9]*$ ]] &&
       [[ $(readlink -f "/proc/$pid/exe" 2>/dev/null) == "$new/replay-control-app" ]] &&
       { curl -kfsS --max-time 3 "https://127.0.0.1:$port/api/version" >/dev/null 2>&1 ||
         curl -fsS --max-time 3 "http://127.0.0.1:$http_port/api/version" >/dev/null 2>&1; }; then
        healthy=true
        break
    fi
    sleep 2
done
if ! $healthy; then exit 1; fi
for old in "$root/releases/"*; do
    if [[ -d $old && $old != "$new" && $old != "$previous" ]]; then rm -rf "$old"; fi
done
trap - EXIT
echo "Development build active: $new"
REMOTE
    STAGE_REMOTE=
    rm -rf "$local_stage"
    STAGE_LOCAL=
    echo "Development build is running on $PI_HOST"
}

main() {
    parse_args "$@"
    if $CLEAN; then
        cargo clean
        if command -v sccache >/dev/null; then sccache --stop-server || true; fi
    fi
    if $RESET; then
        setup_ssh
        trap cleanup EXIT
        remote true
        if remote 'systemctl is-active --quiet replay-control-package-update.service'; then
            echo 'A package update is running on the Pi; retry reset after it finishes' >&2
            exit 1
        fi
        reset_overlay
        return
    fi
    ensure_catalog
    build_wasm
    build_server
    if $BUILD_ONLY; then
        echo 'Fast native build complete'
        return
    fi
    validate_catalog_for_pi
    setup_ssh
    trap cleanup EXIT
    remote true
    deploy "$TARGET_DIR/$TARGET_TRIPLE/$(profile_dir "$SERVER_PROFILE")/$CRATE"
}

main "$@"
