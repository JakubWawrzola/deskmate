#!/bin/sh
# Packs a built Linux daemon into a .deb and a .tar.gz.
#
#   packaging/linux/build.sh <x86_64|aarch64> <deskmate-service binary> <output folder>
#
# The binary must be a release build without the test-hooks feature. The
# version comes from src-tauri/Cargo.toml. dpkg-deb is needed for the .deb;
# without it only the tarball is made. Writes SHA256SUMS.txt for both.
set -eu

if [ "$#" -ne 3 ]; then
    echo "usage: $0 <x86_64|aarch64> <deskmate-service binary> <output folder>" >&2
    exit 2
fi
arch=$1
binary=$2
out=$3

here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)

case "$arch" in
    x86_64) deb_arch=amd64 elf_machine=3e ;;
    aarch64) deb_arch=arm64 elf_machine=b7 ;;
    *)
        echo "unknown architecture: $arch" >&2
        exit 2
        ;;
esac

# e_machine, the two bytes at offset 18 of an ELF header (little endian).
machine=$(od -An -tx1 -j18 -N1 "$binary" | tr -d ' \n')
if [ "$(od -An -c -N4 "$binary" | tr -d ' \n')" != "177ELF" ] || [ "$machine" != "$elf_machine" ]; then
    echo "$binary is not a Linux $arch program" >&2
    exit 1
fi
# Only a test build takes --status-file; a release build calls it wrong
# usage (exit code 2). Asked only when the program runs on this machine.
if "$binary" --version >/dev/null 2>&1; then
    code=0
    "$binary" status --status-file /nonexistent/deskmate-status.json >/dev/null 2>&1 || code=$?
    if [ "$code" -ne 2 ]; then
        echo "$binary is a test build; package a release build" >&2
        exit 1
    fi
fi

version=$(sed -n '/^\[workspace\.package\]/,/^\[/s/^version = "\([^"]*\)"/\1/p' "$repo/src-tauri/Cargo.toml")
if [ -z "$version" ]; then
    echo "cannot read the version from src-tauri/Cargo.toml" >&2
    exit 1
fi

mkdir -p "$out"
out=$(cd "$out" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# ---- tarball ----
name="deskmate-daemon-$version-$arch-linux"
folder="$work/$name"
mkdir -p "$folder"
install -m 0755 "$binary" "$folder/deskmate-service"
install -m 0755 "$here/install.sh" "$folder/install.sh"
for file in deskmate.service deskmate-sleep.service deskmate.sysusers \
    deskmate.tmpfiles 50-deskmate.rules; do
    install -m 0644 "$here/$file" "$folder/$file"
done
install -m 0644 "$repo/LICENSE" "$folder/LICENSE"
install -m 0644 "$repo/docs/LINUX.md" "$folder/LINUX.md"
tar --owner=0 --group=0 --numeric-owner -C "$work" -czf "$out/$name.tar.gz" "$name"

# ---- .deb ----
if command -v dpkg-deb >/dev/null 2>&1; then
    root="$work/deb"
    install -D -m 0755 "$binary" "$root/usr/bin/deskmate-service"
    install -D -m 0644 "$here/deskmate.service" "$root/usr/lib/systemd/system/deskmate.service"
    install -D -m 0644 "$here/deskmate-sleep.service" "$root/usr/lib/systemd/system/deskmate-sleep.service"
    install -D -m 0644 "$here/deskmate.sysusers" "$root/usr/lib/sysusers.d/deskmate.conf"
    install -D -m 0644 "$here/deskmate.tmpfiles" "$root/usr/lib/tmpfiles.d/deskmate.conf"
    install -D -m 0644 "$here/50-deskmate.rules" "$root/usr/share/polkit-1/rules.d/50-deskmate.rules"
    install -D -m 0644 "$repo/LICENSE" "$root/usr/share/doc/deskmate-daemon/copyright"
    install -D -m 0644 "$repo/docs/LINUX.md" "$root/usr/share/doc/deskmate-daemon/LINUX.md"
    mkdir -p "$root/DEBIAN"
    for script in postinst prerm postrm; do
        install -m 0755 "$here/debian/$script" "$root/DEBIAN/$script"
    done
    size=$(du -sk "$root/usr" | cut -f1)
    cat >"$root/DEBIAN/control" <<EOF
Package: deskmate-daemon
Version: $version
Architecture: $deb_arch
Maintainer: Jakub Wawrzola
Installed-Size: $size
Depends: libc6 (>= 2.35), libssl3 | libssl3t64, systemd (>= 245), polkitd | policykit-1
Section: admin
Priority: optional
Homepage: https://github.com/JakubWawrzola/deskmate
Description: Deskmate daemon: this computer in Home Assistant
 Shows this computer in Home Assistant as a device with its sensors (CPU,
 memory, disks, network, uptime, battery, temperatures) and power buttons
 (sleep, hibernate, shut down, restart), through the Deskmate integration.
 Runs as a hardened systemd service under its own system user.
EOF
    dpkg-deb --root-owner-group -Zxz --build "$root" "$out/deskmate-daemon_${version}_${deb_arch}.deb" >/dev/null
else
    echo "dpkg-deb is not installed: no .deb this time" >&2
fi

cd "$out"
sha256sum deskmate-daemon* >SHA256SUMS.txt
cat SHA256SUMS.txt
