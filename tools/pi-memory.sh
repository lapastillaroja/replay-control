#!/bin/bash
# Read memory stats for replay-control on the Pi over SSH.
# Uses the same askpass + StrictHostKeyChecking=no pattern as dev.sh / install.sh.
#
# Usage:
#   tools/pi-memory.sh                           # prints VmRSS / VmHWM / RssAnon / free
#   tools/pi-memory.sh --restart                 # restarts replay-control before reading (clean idle baseline)
#   tools/pi-memory.sh --wait 30                 # wait N seconds after (re)start or before reading, then read
#   tools/pi-memory.sh --ip 192.168.1.50         # override IP/hostname (default: replay.local)
#   tools/pi-memory.sh --json                    # emit a compact JSON line
#
# Environment:
#   PI_USER     ssh user (default: root)
#   PI_PASS     ssh password (default: replayos)
#   PI_IP       Pi address, same as --ip
#
# Exits non-zero on connection failure or if the service PID cannot be read.

set -euo pipefail

PI_USER="${PI_USER:-root}"
PI_PASSWORD="${PI_PASS:-replayos}"
PI_IP="${PI_IP:-replay.local}"
SERVICE="replay-control"
SSH_OPTS="-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR -o ConnectTimeout=10"

RESTART=false
WAIT_SECS=0
JSON=false

while [[ $# -gt 0 ]]; do
    case "$1" in
        --restart)  RESTART=true; shift ;;
        --wait)     WAIT_SECS="$2"; shift 2 ;;
        --ip)       PI_IP="$2"; shift 2 ;;
        --json)     JSON=true; shift ;;
        -h|--help)  sed -n '2,/^$/p' "$0" | sed 's/^# \?//'; exit 0 ;;
        *)          echo "Unknown option: $1" >&2; exit 2 ;;
    esac
done

ASKPASS="$(mktemp)"
trap 'rm -f "$ASKPASS"' EXIT
printf '#!/bin/sh\necho "%s"\n' "$PI_PASSWORD" > "$ASKPASS"
chmod +x "$ASKPASS"

run_ssh() {
    SSH_ASKPASS="$ASKPASS" SSH_ASKPASS_REQUIRE=force DISPLAY= \
        ssh $SSH_OPTS "${PI_USER}@${PI_IP}" "$@"
}

if $RESTART; then
    echo "Restarting $SERVICE on $PI_IP..." >&2
    run_ssh "systemctl restart $SERVICE" >/dev/null
    # Service needs a moment to warm up (DB pools, catalog, etc.)
    [[ "$WAIT_SECS" -lt 5 ]] && WAIT_SECS=5
fi

if [[ "$WAIT_SECS" -gt 0 ]]; then
    sleep "$WAIT_SECS"
fi

# Read the status block in one round-trip.
output=$(run_ssh "\
    PID=\$(systemctl show $SERVICE -p MainPID --value); \
    if [ -z \"\$PID\" ] || [ \"\$PID\" = 0 ]; then \
        echo 'ERROR: no PID for $SERVICE' >&2; exit 1; \
    fi; \
    echo \"PID=\$PID\"; \
    grep -E '^Vm(Peak|Size|RSS|HWM|Data)|^RssAnon' /proc/\$PID/status; \
    echo '--- free ---'; \
    free -m | head -2 \
")

if $JSON; then
    pid=$(awk -F= '/^PID=/{print $2}' <<<"$output")
    rss=$(awk '/^VmRSS/{print $2}' <<<"$output")
    hwm=$(awk '/^VmHWM/{print $2}' <<<"$output")
    anon=$(awk '/^RssAnon/{print $2}' <<<"$output")
    vmsize=$(awk '/^VmSize/{print $2}' <<<"$output")
    vmdata=$(awk '/^VmData/{print $2}' <<<"$output")
    free_mb=$(awk 'NR==3 && $1=="Mem:"{print $4}' <<<"$output")
    ts=$(date -u +%Y-%m-%dT%H:%M:%SZ)
    printf '{"ts":"%s","pid":%s,"vmrss_kb":%s,"vmhwm_kb":%s,"rss_anon_kb":%s,"vmsize_kb":%s,"vmdata_kb":%s,"free_mem_mb":%s}\n' \
        "$ts" "$pid" "$rss" "$hwm" "$anon" "$vmsize" "$vmdata" "${free_mb:-0}"
else
    echo "$output"
fi
