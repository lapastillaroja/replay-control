#!/bin/sh
set -eu

settings_dir=${1:?settings directory required}
mkdir -p "$settings_dir"
if [ ! -e "$settings_dir/settings.cfg" ]; then
    printf 'analytics = "false"\n' > "$settings_dir/settings.cfg"
fi
