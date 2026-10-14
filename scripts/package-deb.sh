#!/usr/bin/env bash
set -euo pipefail

# Build a package from the release artifacts on the pinned Ubuntu runner.
if [[ $# -eq 1 ]]; then
    arch=$1
    version=$(bash "$(dirname "$0")/app-version.sh")
elif [[ $# -eq 2 ]]; then
    version=$1
    arch=$2
else
    echo 'usage: package-deb.sh ARCH | VERSION ARCH' >&2
    exit 1
fi
version=${version#v}
package_version=${version/-/\~}
target_root=${CARGO_TARGET_DIR:-target}
case "$arch" in
    arm64) binary="$target_root/aarch64-unknown-linux-gnu/release/replay-control-app" ;;
    amd64) binary="$target_root/release/replay-control-app" ;;
    *) echo "Unsupported architecture: $arch" >&2; exit 1 ;;
esac
[[ -s "$binary" && -s catalog.sqlite && -d "$target_root/site/pkg" ]] || {
    echo 'Release binary, catalog, or site is missing' >&2
    exit 1
}
# The Ubuntu 22.04 release runner provides glibc 2.35. Reject a binary linked
# on a newer host even if dpkg-shlibdeps
# produces a lower dependency (its symbol database can miss Rust's libc calls).
required_glibc=$(readelf -W --dyn-syms "$binary" |
    awk '$5 != "WEAK" && $7 == "UND" && $8 ~ /@GLIBC_[0-9]+\.[0-9]+/ {
        sub(/.*@GLIBC_/, "", $8); print $8
    }' | sort -Vu | tail -1)
if [[ -z "$required_glibc" ]] || dpkg --compare-versions "$required_glibc" gt 2.35; then
    echo "Binary requires glibc ${required_glibc:-unknown}; rebuild on Ubuntu 22.04 (maximum 2.35)" >&2
    exit 1
fi
"$(dirname "$0")/verify-package-server-routes.sh" "$binary" "$target_root/site"
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
mkdir -p "$stage/DEBIAN" "$stage/usr/bin" "$stage/usr/lib/replay-control" \
    "$stage/usr/share/replay-control" "$stage/usr/share/doc/replay-control" \
    "$stage/usr/share/applications" "$stage/usr/share/icons/hicolor/192x192/apps" \
    "$stage/lib/systemd/system"
install -m755 "$binary" "$stage/usr/bin/replay-control-app"
install -m644 catalog.sqlite "$stage/usr/share/replay-control/catalog.sqlite"
install -m644 LICENSE "$stage/usr/share/doc/replay-control/LICENSE"
cp -a "$target_root/site" "$stage/usr/share/replay-control/site"
install -m755 packaging/start-service "$stage/usr/lib/replay-control/start-service"
install -m755 packaging/open-browser "$stage/usr/lib/replay-control/open-browser"
install -m755 packaging/switch-storage "$stage/usr/lib/replay-control/switch-storage"
install -m755 packaging/package-update-deb "$stage/usr/lib/replay-control/package-update-deb"
install -m644 packaging/replay-control.service "$stage/lib/systemd/system/replay-control.service"
install -m644 packaging/replay-control.desktop "$stage/usr/share/applications/replay-control.desktop"
install -m644 replay-control-app/static/icons/icon-192.png \
    "$stage/usr/share/icons/hicolor/192x192/apps/replay-control.png"
install -m755 packaging/postinst-deb "$stage/DEBIAN/postinst"
install -m755 packaging/preinst-deb "$stage/DEBIAN/preinst"
install -m755 packaging/prerm-deb "$stage/DEBIAN/prerm"
install -m755 packaging/postrm-deb "$stage/DEBIAN/postrm"

# dpkg-shlibdeps derives minimum versions from the actual linked binary,
# including glibc. The runner's cross sysroot is the compatibility baseline.
binary=$(realpath "$stage/usr/bin/replay-control-app")
mkdir -p "$stage/debian"
touch "$stage/debian/control"
shlib_args=()
if [[ "$arch" == arm64 && -d /usr/aarch64-linux-gnu/lib ]]; then
    shlib_args=(-l/usr/aarch64-linux-gnu/lib)
fi
deps=''
shlibdeps_ok=true
if ! deps=$(DEB_HOST_ARCH="$arch" DEB_HOST_MULTIARCH="$(dpkg-architecture -a"$arch" -qDEB_HOST_MULTIARCH)" \
    bash -c 'cd "$1"; shift; dpkg-shlibdeps -O "$@"' _ "$stage" "${shlib_args[@]}" "-e$binary" | sed -n 's/^shlibs:Depends=//p'); then
    shlibdeps_ok=false
fi
# A Fedora cross sysroot has no Debian arm64 symbols database. In that case,
# dpkg-shlibdeps reports Fedora cross packages, which cannot be installed on
# the Pi. Derive minimum symbol versions from the ELF instead; CI's Ubuntu
# cross sysroot takes the normal dpkg-shlibdeps path above.
if [[ "$arch" == arm64 && ( "$shlibdeps_ok" == false || "$deps" == *-cross* || "$deps" == *:arm64* ) ]]; then
    needed=$(readelf -d "$binary" | sed -n 's/.*Shared library: \[\([^]]*\)\].*/\1/p' | sort)
    # Ubuntu's cross linker may record the libc-provided dynamic loader as
    # NEEDED; Fedora's cross linker does not. It needs no separate package.
    needed_without_loader=$(printf '%s\n' "$needed" | sed '/^ld-linux-aarch64\.so\.1$/d')
    expected_needed=$(printf '%s\n' libc.so.6 libgcc_s.so.1 libm.so.6 | sort)
    [[ "$needed_without_loader" == "$expected_needed" ]] || {
        echo "Unknown arm64 shared library requirements: $needed" >&2
        exit 1
    }
    glibc=$(readelf --version-info "$binary" | grep -oE 'GLIBC_[0-9]+\.[0-9]+' | sort -Vu | tail -1)
    libgcc=$(readelf --version-info "$binary" | grep -oE 'GCC_[0-9]+\.[0-9]+' | sort -Vu | tail -1)
    deps="libc6 (>= ${glibc#GLIBC_}), libgcc-s1 (>= ${libgcc#GCC_})"
fi
[[ "$shlibdeps_ok" == true || "$arch" == arm64 ]] || {
    echo 'Could not determine shared library dependencies' >&2
    exit 1
}
[[ -n "$deps" ]] || { echo 'Could not determine shared library dependencies' >&2; exit 1; }
declared_glibc=$(sed -n 's/.*libc6 (>= \([^)]*\)).*/\1/p' <<< "$deps")
[[ -n "$declared_glibc" ]] || { echo 'libc6 dependency is missing' >&2; exit 1; }
if dpkg --compare-versions "$declared_glibc" lt "$required_glibc"; then
    deps=${deps//"libc6 (>= $declared_glibc)"/"libc6 (>= $required_glibc)"}
fi
installed_size=$(du -sk "$stage/usr" "$stage/lib" | awk '{ total += $1 } END { print total }')
cat > "$stage/DEBIAN/control" <<EOF
Package: replay-control
Version: $package_version
Section: web
Priority: optional
Architecture: $arch
Installed-Size: $installed_size
Maintainer: Replay Control <noreply@github.com>
Depends: $deps, systemd, apt, curl, util-linux
Description: Replay Control game library web application
 Independent web UI for managing a ROM library on RePlayOS or Debian.
EOF
dpkg-deb --build --root-owner-group "$stage" "replay-control_${version}_${arch}.deb"
