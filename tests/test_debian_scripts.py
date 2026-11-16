"""Tests for the maintainer scripts of the .deb in packaging/linux/debian.

The scripts run with stand-ins for systemctl, systemd-sysusers and
systemd-tmpfiles that only write down how they were called: nothing is
installed, enabled or started.
"""

from __future__ import annotations

import os
import shutil
import subprocess
from pathlib import Path

import pytest

REPOSITORY_ROOT = Path(__file__).resolve().parent.parent
POSTINST = REPOSITORY_ROOT / "packaging" / "linux" / "debian" / "postinst"
STAND_INS = ("systemctl", "systemd-sysusers", "systemd-tmpfiles")
# The scripts restart the daemon only where systemd runs.
SYSTEMD_RUNS = Path("/run/systemd/system").is_dir()
SHELL = shutil.which("sh")

pytestmark = pytest.mark.skipif(
    SHELL is None or os.name == "nt",
    reason="the maintainer scripts need a POSIX shell",
)


def run_postinst(tmp_path: Path, *arguments: str) -> list[str]:
    """Runs postinst and returns the systemctl calls, one per line."""
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    log = tmp_path / "calls.log"
    for name in STAND_INS:
        stand_in = bin_dir / name
        stand_in.write_text(f'#!/bin/sh\necho "{name} $*" >> "$CALLS"\n')
        stand_in.chmod(0o755)
    subprocess.run(  # noqa: S603 - the script of this repository, with arguments built here
        [str(SHELL), str(POSTINST), *arguments],
        check=True,
        capture_output=True,
        env={"PATH": f"{bin_dir}:/usr/bin:/bin", "CALLS": str(log)},
    )
    calls = log.read_text().splitlines() if log.exists() else []
    return [call for call in calls if call.startswith("systemctl ")]


@pytest.mark.parametrize("arguments", [("configure",), ("configure", "")])
def test_a_first_install_enables_and_starts_the_daemon(
    tmp_path: Path, arguments: tuple[str, ...]
) -> None:
    calls = run_postinst(tmp_path, *arguments)
    assert "systemctl enable deskmate-sleep.service deskmate.service" in calls
    if SYSTEMD_RUNS:
        assert "systemctl restart deskmate.service" in calls


def test_an_upgrade_keeps_a_daemon_the_administrator_turned_off(
    tmp_path: Path,
) -> None:
    calls = run_postinst(tmp_path, "configure", "0.9.0")
    assert not [call for call in calls if call.startswith("systemctl enable")]
    assert "systemctl restart deskmate.service" not in calls
    if SYSTEMD_RUNS:
        assert "systemctl try-restart deskmate.service" in calls
