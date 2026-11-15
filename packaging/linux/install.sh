#!/bin/sh
# Installs the Deskmate daemon from the folder this script is in, or removes
# it again. It copies files and runs systemd's own tools; it downloads
# nothing.
#
#   sudo ./install.sh            install, or update and restart
#   sudo ./install.sh --remove   stop and remove the daemon
#
# The files land where the .deb puts them, so use one or the other.
set -eu

here=$(cd "$(dirname "$0")" && pwd)

BIN=/usr/bin/deskmate-service
UNIT_DIR=/usr/lib/systemd/system
SYSUSERS=/usr/lib/sysusers.d/deskmate.conf
TMPFILES=/usr/lib/tmpfiles.d/deskmate.conf
POLKIT=/usr/share/polkit-1/rules.d/50-deskmate.rules

usage() {
    echo "usage: sudo $0 [--remove]" >&2
}

install_daemon() {
    for file in deskmate-service deskmate.service deskmate-sleep.service \
        deskmate.sysusers deskmate.tmpfiles 50-deskmate.rules; do
        if [ ! -f "$here/$file" ]; then
            echo "$file is missing next to this script" >&2
            exit 1
        fi
    done
    install -D -m 0755 "$here/deskmate-service" "$BIN"
    install -D -m 0644 "$here/deskmate.service" "$UNIT_DIR/deskmate.service"
    install -D -m 0644 "$here/deskmate-sleep.service" "$UNIT_DIR/deskmate-sleep.service"
    install -D -m 0644 "$here/deskmate.sysusers" "$SYSUSERS"
    install -D -m 0644 "$here/deskmate.tmpfiles" "$TMPFILES"
    install -D -m 0644 "$here/50-deskmate.rules" "$POLKIT"
    systemd-sysusers "$SYSUSERS"
    systemd-tmpfiles --create "$TMPFILES"
    systemctl daemon-reload
    systemctl enable deskmate-sleep.service
    systemctl enable deskmate.service
    systemctl restart deskmate.service
    echo "Installed. Pair it with Home Assistant: sudo deskmate-service pair"
}

remove_daemon() {
    systemctl disable --now deskmate.service 2>/dev/null || true
    systemctl disable deskmate-sleep.service 2>/dev/null || true
    rm -f "$BIN" "$UNIT_DIR/deskmate.service" "$UNIT_DIR/deskmate-sleep.service" \
        "$SYSUSERS" "$TMPFILES" "$POLKIT"
    systemctl daemon-reload
    echo "Removed. Settings and keys are still in /etc/deskmate and /var/lib/deskmate;"
    echo "delete them with: sudo rm -r /etc/deskmate /var/lib/deskmate"
}

if [ "$(id -u)" -ne 0 ]; then
    echo "run this as root: sudo $0 $*" >&2
    exit 5
fi
if [ ! -d /run/systemd/system ]; then
    echo "the Deskmate daemon needs systemd, which is not running here" >&2
    exit 1
fi

case "${1:-}" in
    "") install_daemon ;;
    --remove) remove_daemon ;;
    *)
        usage
        exit 2
        ;;
esac
