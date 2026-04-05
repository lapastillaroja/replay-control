#!/bin/sh
# Mock systemctl for container testing.
#
# Handles start/stop/restart/is-active for the replay-control service.
# Manages the app process via a PID file. All other commands are no-ops.

PIDFILE="/var/run/replay-control.pid"
APP_BIN="/usr/local/bin/replay-control-app"
ENV_FILE="/etc/default/replay-control"

# Source environment if available
if [ -f "$ENV_FILE" ]; then
    . "$ENV_FILE"
fi
PORT="${PORT:-8080}"

get_pid() {
    if [ -f "$PIDFILE" ] && kill -0 "$(cat "$PIDFILE")" 2>/dev/null; then
        cat "$PIDFILE"
    else
        echo ""
    fi
}

do_start() {
    pid=$(get_pid)
    if [ -n "$pid" ]; then
        echo "replay-control already running (pid $pid)"
        return 0
    fi
    $APP_BIN --port "$PORT" --site-root /usr/local/share/replay/site --storage-path /media/usb &
    echo $! > "$PIDFILE"
    echo "Started replay-control (pid $!)"
}

do_stop() {
    pid=$(get_pid)
    if [ -n "$pid" ]; then
        kill "$pid" 2>/dev/null || true
        # Wait for process to exit
        for i in $(seq 1 10); do
            if ! kill -0 "$pid" 2>/dev/null; then
                break
            fi
            sleep 0.5
        done
        # Force kill if still alive
        kill -9 "$pid" 2>/dev/null || true
        rm -f "$PIDFILE"
        echo "Stopped replay-control (pid $pid)"
    else
        echo "replay-control is not running"
    fi
}

do_restart() {
    do_stop
    sleep 0.5
    do_start
}

do_is_active() {
    pid=$(get_pid)
    if [ -n "$pid" ]; then
        echo "active"
        return 0
    else
        echo "inactive"
        return 3
    fi
}

# Parse arguments
ACTION="$1"
SERVICE="$2"

# Only handle replay-control service
if [ "$SERVICE" != "replay-control" ] && [ "$SERVICE" != "replay-control.service" ]; then
    # No-op for other services
    exit 0
fi

case "$ACTION" in
    start)
        do_start
        ;;
    stop)
        do_stop
        ;;
    restart)
        do_restart
        ;;
    is-active)
        do_is_active
        ;;
    status)
        pid=$(get_pid)
        if [ -n "$pid" ]; then
            echo "replay-control.service - RePlay Control App"
            echo "   Active: active (running)"
            echo "   PID: $pid"
        else
            echo "replay-control.service - RePlay Control App"
            echo "   Active: inactive (dead)"
        fi
        ;;
    *)
        # No-op for unsupported commands (enable, disable, daemon-reload, etc.)
        exit 0
        ;;
esac
