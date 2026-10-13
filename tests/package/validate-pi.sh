#!/usr/bin/env bash
set -euo pipefail

# Exercise a real systemd-run/APT upgrade on a RePlayOS Pi. The temporary
# lower-version package contains the same release payload as the final .deb.
package=${1:?usage: tests/package/validate-pi.sh replay-control_VERSION_arm64.deb [host]}
host=${2:-replay.local}
package=$(realpath "$package")
version=$(dpkg-deb --field "$package" Version)
app_version=${version/\~/-}
[[ $(dpkg-deb --field "$package" Package) == replay-control &&
   $(dpkg-deb --field "$package" Architecture) == arm64 ]] || {
    echo 'Expected a Replay Control arm64 Debian package' >&2
    exit 1
}
checksum=$(sha256sum "$package" | awk '{print $1}')
askpass=$(mktemp)
trap 'rm -f "$askpass"' EXIT
printf '#!/bin/sh\necho "%s"\n' "${PI_PASS:-replayos}" > "$askpass"
chmod 700 "$askpass"
export SSH_ASKPASS="$askpass" SSH_ASKPASS_REQUIRE=force
ssh_opts=(-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR)
scp "${ssh_opts[@]}" "$package" "root@$host:/root/replay-control-validation.deb"
ssh "${ssh_opts[@]}" "root@$host" bash -s -- "$version" "$app_version" "$checksum" <<'REMOTE'
set -euo pipefail
trap 'echo "Pi validation failed at line $LINENO" >&2' ERR
version=$1
app_version=$2
checksum=$3
final=/root/replay-control-validation.deb
stage=''
settings_file=/etc/replay-control/settings.cfg
helper_unit=replay-control-package-validation.service
result_backup=''
had_result=false
cleanup() {
    status=$?
    trap - EXIT
    [[ -z "$stage" ]] || rm -rf "$stage"
    systemctl stop "$helper_unit" >/dev/null 2>&1 || true
    if (( status != 0 )); then
        installed=$(dpkg-query -W -f='${Version}' replay-control 2>/dev/null || true)
        if [[ "$installed" != "$version" ]]; then
            if ! DEBIAN_FRONTEND=noninteractive apt-get install -y "$final"; then
                echo "Automatic recovery failed; install $final with APT on the Pi." >&2
            fi
        fi
        if ! systemctl is-active --quiet replay-control.service ||
           ! curl -fsS --max-time 3 http://127.0.0.1:8080/api/version >/dev/null 2>&1; then
            if ! systemctl restart replay-control.service; then
                echo 'Automatic service restart failed; check systemctl status replay-control.service.' >&2
            fi
        fi
    fi
    rm -f /root/replay-control-validation-old.deb
    rm -f /var/tmp/replay-control-validation-update.deb
    if [[ -n "$result_backup" ]]; then
        exec 9>/var/lib/replay-control/update-result.lock
        flock -x 9
        if $had_result; then
            cp -a "$result_backup" /var/lib/replay-control/update-result
        else
            rm -f /var/lib/replay-control/update-result
        fi
        flock -u 9
        rm -f "$result_backup"
    fi
    exit "$status"
}
printf '%s  %s\n' "$checksum" "$final" | sha256sum -c -
replay_before=$(dpkg-query -W -f='${Version}' replay-system)
unit_path=$(systemctl show -p FragmentPath --value replay-control.service)
[[ "$unit_path" == /usr/lib/systemd/system/replay-control.service ||
   "$unit_path" == /lib/systemd/system/replay-control.service ]] || {
    echo "Pi is still using an archive service unit ($unit_path); migrate with install.sh first." >&2
    exit 1
}
test -s /etc/replay-control/settings.cfg
analytics=$(sed -n 's/^[[:space:]]*analytics[[:space:]]*=[[:space:]]*"\([^"]*\)"[[:space:]]*$/\1/p' "$settings_file" | tail -1)
if [[ $analytics != false ]]; then
    echo 'Disable analytics in Replay Control Settings before Pi package validation.' >&2
    exit 1
fi
test -s /media/sd/.replay-control/user_data.db
settings_before=$(sha256sum "$settings_file" | cut -d' ' -f1)
user_data_before=$(python3 -c 'import hashlib, sqlite3; db = sqlite3.connect("file:/media/sd/.replay-control/user_data.db?mode=ro", uri=True); print(hashlib.sha256("\n".join(db.iterdump()).encode()).hexdigest())')
mapfile -t library_dbs < <(find /var/lib/replay-control/storages -mindepth 2 -maxdepth 2 -name library.db -type f)
(( ${#library_dbs[@]} > 0 ))
for db in "${library_dbs[@]}"; do test -s "$db"; done
if systemctl is-active --quiet replay-control-package-update.service; then
    echo 'Replay Control package update is already running; retry validation later.' >&2
    exit 1
fi
if systemctl is-active --quiet "$helper_unit"; then
    echo 'A previous package validation is still running; retry later.' >&2
    exit 1
fi
trap cleanup EXIT

stage=$(mktemp -d)
dpkg-deb --raw-extract "$final" "$stage"
sed -i "s/^Version:.*/Version: ${version}~pi-validation1/" "$stage/DEBIAN/control"
dpkg-deb --build --root-owner-group "$stage" /root/replay-control-validation-old.deb
rm -rf "$stage"
stage=''
DEBIAN_FRONTEND=noninteractive apt-get install -y --allow-downgrades \
    /root/replay-control-validation-old.deb
test "$(dpkg-query -W -f='${Version}' replay-control)" = "${version}~pi-validation1"
before_pid=$(systemctl show -p MainPID --value replay-control.service)
cp "$final" /var/tmp/replay-control-validation-update.deb
backup_temp=$(mktemp)
exec 9>/var/lib/replay-control/update-result.lock
flock -x 9
if [[ -f /var/lib/replay-control/update-result ]]; then
    cp -a /var/lib/replay-control/update-result "$backup_temp"
    had_result=true
fi
result_backup=$backup_temp
rm -f /var/lib/replay-control/update-result
flock -u 9
systemd-run --unit="$helper_unit" --collect --no-block \
    /usr/lib/replay-control/package-update-deb \
    /var/tmp/replay-control-validation-update.deb "$app_version"
result=''
for _ in {1..90}; do
    result=$(cat /var/lib/replay-control/update-result 2>/dev/null || true)
    if [[ "$result" == "ok:$app_version" ]]; then break; fi
    if [[ "$result" == failed:* ]]; then echo "$result" >&2; exit 1; fi
    sleep 2
done
[[ "$result" == "ok:$app_version" ]] || {
    echo "Package helper did not finish: $result" >&2
    exit 1
}
test "$(dpkg-query -W -f='${Version}' replay-control)" = "$version"
test "$(dpkg-query -W -f='${Version}' replay-system)" = "$replay_before"
after_pid=$(systemctl show -p MainPID --value replay-control.service)
[[ "$after_pid" != "$before_pid" && "$after_pid" =~ ^[0-9]+$ ]]
test "$(readlink "/proc/$after_pid/exe")" = /usr/bin/replay-control-app
allowed_cpus=$(awk '/^Cpus_allowed_list:/ { print $2 }' "/proc/$after_pid/status")
[[ -n "$allowed_cpus" && "$allowed_cpus" != 0 &&
   "$allowed_cpus" != 0,* && "$allowed_cpus" != 0-* ]]
systemctl is-active --quiet replay-control.service
curl -fsS http://127.0.0.1:8080/api/version | grep -q "\"version\":\"$app_version\""
curl -kfsS https://127.0.0.1:8443/api/version | grep -q "\"version\":\"$app_version\""
dpkg -V replay-control
test -s /etc/replay-control/settings.cfg
test -s /media/sd/.replay-control/user_data.db
test "$(sha256sum /etc/replay-control/settings.cfg | cut -d' ' -f1)" = "$settings_before"
test "$(python3 -c 'import hashlib, sqlite3; db = sqlite3.connect("file:/media/sd/.replay-control/user_data.db?mode=ro", uri=True); print(hashlib.sha256("\n".join(db.iterdump()).encode()).hexdigest())')" = "$user_data_before"
for db in "${library_dbs[@]}"; do test -s "$db"; done
echo "Pi package update passed: $version; replay-system remained $replay_before"
REMOTE
