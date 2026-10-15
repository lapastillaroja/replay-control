# Package integration tests

Fresh test images set `analytics = "false"` in `settings.cfg`. Pi validation checks the existing preference and asks you to disable analytics in Settings before running if it is enabled.

Build instructions are in [Package Distribution](../../docs/architecture/package-distribution.md#building-packages-locally).

Run the pytest-driven package scenarios after building the packages. The small entry points select the package format and architecture:

```sh
tests/package/run-debian.sh arm64 replay-control_VERSION_arm64.deb
tests/package/run-debian.sh amd64 replay-control_VERSION_amd64.deb
tests/package/run-fedora.sh x86_64 replay-control-VERSION-1.x86_64.rpm
tests/package/validate-pi.sh replay-control_VERSION_arm64.deb [pi-host]
```

If Playwright's Chromium browser is installed, set `RUN_BROWSER_E2E=1` on the amd64 or Fedora command to check that the packaged HTTPS site hydrates, its server function routes match the native binary, and the standalone checklist shows storage followed by metadata. Fedora checks that the startup pipeline enters the automatic metadata fetch path. Debian uses a small local metadata source and checks that startup imports it into the host data directory. The amd64 run also checks that the post-reconnect update page displays both success and actionable failure results.

CI builds amd64 Debian and Fedora packages from its build artifacts and runs both pytest scenarios with `RUN_BROWSER_E2E=1`. It also builds, installs, and smoke-tests the Flatpak on Ubuntu 24.04. The Flatpak test uses app-owned folders and does not exercise the graphical portal chooser. The arm64 suite remains available for local validation. Container commands and HTTP assertions live in `support.py` and `scenarios.py`; only the v1 archive fixture, service shims, and Pi validator remain shell scripts.

The arm64 test uses Debian 13 (`trixie`), the userspace observed on the RePlayOS test Pi. It downloads and checksum-verifies the official RePlayOS frontend and library `.deb` packages, installs them in the integration image, then tests a clean Replay Control package install and migration from the published Replay Control 1.0 archive installer. It also tests a standalone install without initial storage configuration, the full Update Now server function against a local release fixture, a real 1.0-to-current package upgrade through the APT helper, two successive dpkg revision upgrades, uninstall with settings preserved, purge, and APT failure/interruption reporting. The amd64 Debian and Fedora 42 tests exercise first-run storage selection, the setup checklist, application data paths, service restarts, and a real package metadata upgrade through the full Update Now path using APT and DNF, respectively. Debian also tests a second storage switch and rollback. The fixture's 0.0.1 package has the current binary, so these tests verify the package-manager transaction and service restart; the separate arm64 1.0-to-current case verifies a real binary version transition.

The RePlayOS frontend `.deb` provides `/opt/replay`; the full `replay-system` metapackage also requires Raspberry Pi kernel and boot packages that do not belong in a generic container. Rootless arm64 emulation cannot start services through systemd cgroups here, so the tests use `systemctl-shim` to launch the real binaries. QEMU reports itself through `/proc/<pid>/exe`, so `readlink-shim` exposes the guest binary path to the migration check. Validate the real unit and process identity on a Pi using `install.sh --package FILE --ip replay.local`, then check `systemctl status replay-control` and `/api/version`.

For a local rerun against an already built frontend image, set `PACKAGE_TEST_REPLAYOS_IMAGE=replay-control-package-test:replayos-arm64`. Normal runs fetch and verify the current frontend packages.

`validate-pi.sh` stages the final package on the Pi, installs a temporary lower metadata version containing the same binary and assets, then upgrades through the real transient systemd helper. It checks the service PID, HTTP and HTTPS version endpoints, package integrity, preserved settings and databases, and an unchanged `replay-system` version. The final `.deb` remains in `/root/replay-control-validation.deb` for recovery if the helper fails.
Run `install.sh --package PACKAGE --ip replay.local` first when the Pi still has the archive service unit; `validate-pi.sh` refuses to upgrade through that unit because its executable is not package-owned.
