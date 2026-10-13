Name: replay-control
Version: %{version}
Release: 1%{?dist}
Summary: Replay Control game library web application
License: GPL-3.0-or-later
URL: https://github.com/lapastillaroja/replay-control
AutoReqProv: yes
Requires: /usr/bin/systemctl, /usr/bin/curl, /usr/bin/dnf, /usr/bin/taskset, /usr/bin/flock

%description
Independent web UI for managing a ROM library on Fedora.

%prep

%build

%install
mkdir -p %{buildroot}
cp -a %{payload}/. %{buildroot}/

%post
mkdir -p /etc/replay-control /var/lib/replay-control/storages
if [ ! -e /etc/default/replay-control ]; then
    cat > /etc/default/replay-control <<'EOF'
REPLAY_PORT=8080
REPLAY_HTTPS_PORT=8443
# Optional: preselect a standalone storage root containing roms/.
#REPLAY_STORAGE_PATH=/path/to/storage
EOF
fi
systemctl daemon-reload >/dev/null 2>&1 || :
systemctl enable replay-control.service >/dev/null 2>&1 || :

%posttrans
systemctl restart replay-control.service >/dev/null 2>&1 || :

%preun
if [ "$1" -eq 0 ]; then
    systemctl stop replay-control.service >/dev/null 2>&1 || :
    systemctl disable replay-control.service >/dev/null 2>&1 || :
fi

%postun
systemctl daemon-reload >/dev/null 2>&1 || :

%files
%defattr(-,root,root,-)
/usr/bin/replay-control-app
/usr/lib/replay-control/start-service
/usr/lib/replay-control/open-browser
/usr/lib/replay-control/switch-storage
/usr/lib/replay-control/package-update-rpm
/usr/lib/systemd/system/replay-control.service
/usr/share/applications/replay-control.desktop
/usr/share/icons/hicolor/192x192/apps/replay-control.png
%license /usr/share/licenses/replay-control/LICENSE
/usr/share/replay-control/catalog.sqlite
/usr/share/replay-control/site
