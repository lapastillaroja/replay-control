# Package Distribution

Release builds produce `arm64` and `amd64` Debian packages, an `x86_64` Fedora RPM, and an experimental `x86_64` Flatpak alongside the existing arm64 archives and `install.sh`. The Debian and RPM packages own the binary, catalog, site assets, service unit, service/update helpers, and a desktop launcher with its icon. The launcher reads `REPLAY_HTTPS_PORT` from `/etc/default/replay-control`, then uses `xdg-open` or `gio open` to open the local app in the desktop's default browser. Headless installations need no desktop dependency. Settings in `/etc/default/replay-control` and `/etc/replay-control`, plus `/var/lib/replay-control` and storage data, remain outside package ownership. No RePlayOS package or version is declared as a dependency.

The release runner is pinned to Ubuntu 22.04 for a stable cross-compilation baseline. `dpkg-shlibdeps` derives shared-library dependency versions from the linked binary; on cross-build hosts without a usable Debian symbols database, the packager derives the minimum glibc and libgcc symbol versions from the ELF instead. The package additionally requires APT, systemd, and curl. RPM dependencies are generated from the ELF binary and the required service/update commands. Debian 13 is the supported package baseline; an older glibc build alone does not establish compatibility with older distributions. The arm64 end-to-end container uses Debian 13, matching the current RePlayOS Pi's userspace, and installs the official RePlayOS frontend `.deb` before testing Replay Control migration.

The service wrapper reads `/etc/default/replay-control` and passes an absolute catalog and site path. Package installs enable HTTPS on port 8443 by default, including standalone hosts; port 8080 points browsers to HTTPS. Both archive and package service installs use `REPLAY_PORT` and `REPLAY_HTTPS_PORT`; new services ignore `REPLAY_EXTRA_ARGS`. The package migration still reads that legacy value to locate v1 archive data. Outside RePlayOS, `REPLAY_STORAGE_PATH` optionally preselects a storage root. A selection made in the browser is stored under `/var/lib/replay-control/standalone-storage-path` and takes precedence on later starts. Standalone host settings and databases stay under `/etc/replay-control` and `/var/lib/replay-control` as the selected storage root changes. On RePlayOS, storage always follows `replay.cfg`; a legacy storage override cannot change the host type or selected storage. Package upgrades preserve settings and data. `install.sh` defaults to the archive method. With explicit `--package`, it verifies a release package against `checksums.sha256`, checks package metadata and APT dependency eligibility, and migrates an archive installation by switching its legacy unit only after APT succeeds. For v1 archives using an explicit storage path, the installer copies its host data and settings into the global directories before the packaged service starts. It health-checks the packaged service before deleting legacy application files. Direct APT installation over an archive service unit is refused so the migration cannot be skipped.

A browser storage-folder selector belongs only to standalone packages. Host detection uses `/opt/replay` and is independent of any storage path. The selector routes require a loopback socket peer. Without an initial path, a small HTTPS bootstrap router starts before `AppState`; an unavailable selected mount opens the same selector for recovery. The bootstrap router also serves `/api/version` and `/api/update-result` so the updating page can finish after a restart into recovery; `/settings` redirects to the selector in this state. The setup card then shows storage before metadata. A separate systemd helper persists later storage changes, restarts the service, and restores the previous choice on failure or interruption. It never moves data between roots.

The Leptos server function route hash includes the build's checkout path and can differ across Rust toolchains. The packagers verify that every route called by the browser WASM exists in the native binary; release builds must produce both artifacts from the same checkout path and toolchain.

## Building packages locally

Packages are the standard developer artifact for installation and integration tests. The public RePlayOS installer still defaults to archives; package installation is opt-in. Build the binary, site, and catalog together before packaging. On an x86_64 host with `rpmbuild` and Python 3 installed, create the Fedora RPM with:

```sh
./build.sh amd64
./scripts/package-rpm.sh
```

The result is `replay-control-VERSION-1.x86_64.rpm` in the repository root. On the Ubuntu 22.04 package baseline, `./scripts/package-deb.sh amd64` packages the same build as a Debian package. For arm64, build with `./build.sh aarch64` and package with `./scripts/package-deb.sh arm64` using the cross-toolchain and sysroot described in the release workflow. Both package scripts read the version from `replay-control-app/Cargo.toml` by default. Release automation passes a version explicitly after checking it against the tag.

`dev.sh --pi` builds the current code with fast development profiles and deploys it on package or archive installations. On a clean Pi it uses `install.sh` once to bootstrap the archive service from those fast artifacts and initializes `analytics = "false"` only if settings do not exist. Later deploys preserve the user's choice. The built binary, catalog, site, and service wrapper are staged under `/var/lib/replay-control/dev`; a systemd drop-in switches the service to the staged build after transfer. Package-owned files stay intact. A failed start restores the previous build. `dev.sh --pi --reset` removes the development override and returns to the installed service. The wrapper marks development builds with `REPLAY_CONTROL_DEV_MODE=1` to reject Update Now. Package test images initialize the same analytics preference; Pi package validation requires it to be disabled. Use `install.sh --package FILE` to install a prebuilt release package.

For packaged installs, Update Now downloads the matching package, requires a checksum manifest, validates package identity/version/architecture, then launches a separate transient systemd unit. That unit installs via APT or DNF and writes a durable result under `/var/lib/replay-control`. The browser polls the result and running version after the app restarts. Archive installations retain the archive swap path. Package files are never overwritten directly by the archive updater.

GitHub release packages do not constitute an APT or DNF repository. Offline RePlayOS updates do not include Replay Control; its release cycle remains independent.

## Desktop Flatpak

The experimental x86_64 Flatpak is a standalone desktop install. It includes the same native server, site, and catalog, but runs as the desktop user and opens the local HTTPS app in the default browser. The server binds only to loopback on ports 18080 and 18443. It uses the user's Flatpak XDG data and config directories for host data and settings; user data tied to a library remains at the selected storage root. The storage selector requests a folder through the desktop file chooser portal, saves the granted path when the root is writable and contains a readable `roms/` directory, and restarts the server within the launcher process. The manifest grants network access but no general host filesystem access. The app's Update Now action directs users to a newer release bundle instead of replacing files in `/app`. A GitHub bundle is not an update repository.

Build with `./build.sh amd64` and `./scripts/package-flatpak.sh` after installing `flatpak` and `flatpak-builder` and adding the Flathub remote. The result is a single-file bundle in the repository root. Recipients also need the `org.freedesktop.Platform//25.08` runtime; the bundle records Flathub as its runtime source.

RePlayOS's menu wrappers download `main/install.sh` when selected. Install passes through its arguments; uninstall invokes it with `--uninstall`. The menu does not add `--package` or `--yes`. The package-aware installer must be on `main` when package downloads become available, or the menu could run an older installer that cannot remove a package and may replace its service unit with an archive one.

## Configuration variables

The installer reads these variables when invoked:

| Variable | Default | Purpose |
|---|---|---|
| `PI_PASS` | `replayos` | Root password for an SSH installation; equivalent to `--pi-pass`. |
| `REPLAY_PI_ADDR` | Pi discovery | Pi address; equivalent to `--ip`. |
| `REPLAY_CONTROL_VERSION` | `latest` | Release tag, `latest`, or `beta`; equivalent to `--version`. |

The service reads `/etc/default/replay-control` at startup:

| Variable | Default | Purpose |
|---|---|---|
| `REPLAY_PORT` | `8080` | HTTP guidance port when HTTPS is enabled. |
| `REPLAY_HTTPS_PORT` | `8443` | HTTPS app and API port. The desktop launcher and package update health check use this port. |
| `REPLAY_STORAGE_PATH` | Unset | Optional initial storage root for a standalone package. A saved browser choice takes precedence. RePlayOS ignores this value. |
| `REPLAY_SITE_ROOT` | `/usr/local/share/replay/site` | Archive service assets; package services use their packaged site path. |
| `REPLAY_CONTROL_IDENTITY_WORKERS` | `2` | Workers for library identity matching; valid range 1–4. |
| `RUST_LOG` | `replay_control_app=info,replay_control_core=info` | Log filter read at service startup. |

`REPLAY_EXTRA_ARGS` is read only while migrating a v1 archive installation, to locate its old data and settings. New service units do not pass it to the app. `REPLAY_CONFIG_PATH` is not an active setting.
