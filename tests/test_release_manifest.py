"""Tests for scripts/make_release_manifest.py.

They need neither Home Assistant nor the integration: every installer,
signature and key below is made up in the test and used nowhere else.
"""

from __future__ import annotations

import base64
import hashlib
import importlib.util
import json
import re
from datetime import UTC, datetime
from pathlib import Path
from types import ModuleType

import pytest

REPOSITORY_ROOT = Path(__file__).resolve().parent.parent
VERSION = "0.9.0"
KEY_ID = bytes(range(1, 9))
OTHER_KEY_ID = bytes(range(11, 19))
PUBLISHED = datetime(2026, 11, 14, 12, 30, 0, tzinfo=UTC)
# The address rule of the app (src-tauri/src/updates/policy.rs).
ALLOWED_ADDRESS = re.compile(
    r"https://github\.com/JakubWawrzola/deskmate/releases/download/"
    r"[A-Za-z0-9._-]+/[A-Za-z0-9._-]+"
)


def load_script() -> ModuleType:
    source = REPOSITORY_ROOT / "scripts" / "make_release_manifest.py"
    spec = importlib.util.spec_from_file_location("make_release_manifest", source)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


script = load_script()


def encode(data: bytes) -> str:
    return base64.b64encode(data).decode()


def sample_signature(
    name: str,
    version: str | None = VERSION,
    key_id: bytes = KEY_ID,
    algorithm: bytes = b"ED",
) -> str:
    """A `.sig` file as `tauri build` writes it, with a made-up signature."""
    comment = f"timestamp:1800000000\tfile:{name}"
    if version is not None:
        comment += f"\tversion:{version}"
    text = "\n".join(
        [
            "untrusted comment: signature from tauri secret key",
            encode(algorithm + key_id + bytes(64)),
            f"trusted comment: {comment}",
            encode(bytes(64)),
        ]
    )
    return encode(text.encode())


class Release:
    """A folder with two sample installers, their signatures and a config."""

    def __init__(self, folder: Path) -> None:
        self.folder = folder
        self.out_dir = folder / "out"
        self.out_dir.mkdir()
        self.config = folder / "tauri.conf.json"
        public_key = "untrusted comment: minisign public key: TEST\n" + encode(
            b"Ed" + KEY_ID + bytes(32)
        )
        self.config.write_text(
            json.dumps(
                {"plugins": {"updater": {"pubkey": encode(public_key.encode())}}}
            ),
            encoding="utf-8",
        )
        self.installers = {}
        self.signatures = {}
        for architecture in ("x64", "arm64"):
            name = script.installer_name(VERSION, architecture)
            installer = folder / name
            installer.write_bytes(b"MZ" + architecture.encode() * 1000)
            signature = folder / f"{name}.sig"
            signature.write_text(sample_signature(name), encoding="utf-8")
            self.installers[architecture] = installer
            self.signatures[architecture] = signature

    def make(self, **changes: object) -> tuple[Path, Path]:
        arguments: dict[str, object] = {
            "version": VERSION,
            "installers": self.installers,
            "signatures": self.signatures,
            "notes": "Signed automatic updates.",
            "out_dir": self.out_dir,
            "config": self.config,
            "published": PUBLISHED,
        }
        arguments.update(changes)
        return script.make_release(**arguments)

    def command_line(self) -> list[str]:
        return [
            "--version",
            VERSION,
            "--x64",
            str(self.installers["x64"]),
            "--arm64",
            str(self.installers["arm64"]),
            "--out-dir",
            str(self.out_dir),
            "--config",
            str(self.config),
        ]

    def outputs(self) -> list[str]:
        return sorted(path.name for path in self.out_dir.iterdir())


@pytest.fixture
def release(tmp_path: Path) -> Release:
    return Release(tmp_path)


def test_writes_the_manifest_the_updater_reads(release: Release) -> None:
    manifest_path, _ = release.make()

    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    assert manifest_path.name == "latest.json"
    assert manifest["version"] == VERSION
    assert manifest["notes"] == "Signed automatic updates."
    assert manifest["pub_date"] == "2026-11-14T12:30:00Z"
    assert set(manifest["platforms"]) == {"windows-x86_64", "windows-aarch64"}
    x64 = manifest["platforms"]["windows-x86_64"]
    arm64 = manifest["platforms"]["windows-aarch64"]
    assert x64["url"] == (
        "https://github.com/JakubWawrzola/deskmate/releases/download/"
        "v0.9.0/Deskmate_0.9.0_x64-setup.exe"
    )
    assert arm64["url"] == (
        "https://github.com/JakubWawrzola/deskmate/releases/download/"
        "v0.9.0/Deskmate_0.9.0_arm64-setup.exe"
    )
    assert x64["signature"] == release.signatures["x64"].read_text(encoding="utf-8")
    assert arm64["signature"] == release.signatures["arm64"].read_text(encoding="utf-8")
    assert x64["signature"] != arm64["signature"]
    for platform in manifest["platforms"].values():
        assert set(platform) == {"signature", "url"}
        assert ALLOWED_ADDRESS.fullmatch(platform["url"])


def test_writes_the_checksums_of_both_installers(release: Release) -> None:
    _, checksums_path = release.make()

    assert checksums_path.name == "SHA256SUMS.txt"
    lines = checksums_path.read_text(encoding="utf-8").splitlines()
    assert lines == [
        f"{hashlib.sha256(release.installers[architecture].read_bytes()).hexdigest()}"
        f"  {release.installers[architecture].name}"
        for architecture in ("arm64", "x64")
    ]


def test_refuses_to_replace_an_existing_file(release: Release) -> None:
    manifest_path, checksums_path = release.make()
    first = manifest_path.read_text(encoding="utf-8")

    with pytest.raises(script.ManifestError, match="already exists"):
        release.make(notes="Another text.")
    assert manifest_path.read_text(encoding="utf-8") == first

    # One leftover file is enough to refuse, and nothing is written next to it.
    manifest_path.unlink()
    with pytest.raises(script.ManifestError, match="already exists"):
        release.make()
    assert release.outputs() == [checksums_path.name]


def test_makes_the_folder_for_the_release(release: Release) -> None:
    out_dir = release.folder / VERSION
    release.make(out_dir=out_dir)
    assert sorted(path.name for path in out_dir.iterdir()) == [
        "SHA256SUMS.txt",
        "latest.json",
    ]


def test_refuses_a_folder_whose_parent_does_not_exist(release: Release) -> None:
    with pytest.raises(script.ManifestError, match="no such folder"):
        release.make(out_dir=release.folder / "missing" / VERSION)
    assert not (release.folder / "missing").exists()


def test_refuses_a_file_in_place_of_the_folder(release: Release) -> None:
    taken = release.folder / "taken"
    taken.write_text("not a folder", encoding="utf-8")
    with pytest.raises(script.ManifestError, match="not a folder"):
        release.make(out_dir=taken)


def test_a_refused_release_leaves_no_folder_behind(release: Release) -> None:
    out_dir = release.folder / VERSION
    release.signatures["x64"].unlink()
    with pytest.raises(script.ManifestError, match="no such file"):
        release.make(out_dir=out_dir)
    assert not out_dir.exists()


@pytest.mark.parametrize(
    "version",
    [
        "",
        "0.9",
        "v0.9.0",
        "0.9.0.1",
        "0.9.0-beta.1",
        "0.9.0+build",
        "00.9.0",
        "0.9.x",
        "latest",
    ],
)
def test_refuses_a_version_the_app_would_not_install(
    release: Release, version: str
) -> None:
    with pytest.raises(script.ManifestError, match="major.minor.patch"):
        release.make(version=version)
    assert release.outputs() == []


def test_refuses_installers_given_for_the_wrong_architecture(release: Release) -> None:
    swapped = {"x64": release.installers["arm64"], "arm64": release.installers["x64"]}
    with pytest.raises(script.ManifestError, match="expected a file named"):
        release.make(installers=swapped)
    assert release.outputs() == []


def test_refuses_an_installer_of_another_version(release: Release) -> None:
    with pytest.raises(script.ManifestError, match="expected a file named"):
        release.make(version="0.9.1")


def test_refuses_a_missing_installer(release: Release) -> None:
    release.installers["arm64"].unlink()
    with pytest.raises(script.ManifestError, match="no such file"):
        release.make()
    assert release.outputs() == []


def test_refuses_a_file_that_is_not_a_windows_program(release: Release) -> None:
    release.installers["x64"].write_bytes(b"PK\x03\x04 a zip archive")
    with pytest.raises(script.ManifestError, match="not a Windows program"):
        release.make()

    release.installers["x64"].write_bytes(b"")
    with pytest.raises(script.ManifestError, match="is empty"):
        release.make()
    assert release.outputs() == []


def test_refuses_an_installer_the_app_would_not_download(release: Release) -> None:
    with release.installers["x64"].open("r+b") as installer:
        installer.truncate(script.MAX_INSTALLER_BYTES + 1)
    with pytest.raises(script.ManifestError, match="larger than"):
        release.make()


def test_refuses_a_missing_signature(release: Release) -> None:
    release.signatures["x64"].unlink()
    with pytest.raises(script.ManifestError, match="no such file"):
        release.make()
    assert release.outputs() == []


@pytest.mark.parametrize(
    "content",
    [
        "",
        "not base64 at all!",
        encode(b"\xff\xfe not text"),
        encode(b"one line only"),
        encode(b"untrusted comment: x\nAAAA\ntrusted comment: y\nAAAA"),
    ],
)
def test_refuses_a_file_that_is_not_a_signature(release: Release, content: str) -> None:
    release.signatures["arm64"].write_text(content, encoding="utf-8")
    with pytest.raises(script.ManifestError, match="signature"):
        release.make()
    assert release.outputs() == []


def test_refuses_a_signature_for_another_file(release: Release) -> None:
    release.signatures["x64"].write_text(
        sample_signature(release.installers["arm64"].name), encoding="utf-8"
    )
    with pytest.raises(script.ManifestError, match="signs"):
        release.make()


@pytest.mark.parametrize("signed_version", ["0.8.5", "0.9.1", None])
def test_refuses_a_signature_for_another_version(
    release: Release, signed_version: str | None
) -> None:
    name = release.installers["x64"].name
    release.signatures["x64"].write_text(
        sample_signature(name, version=signed_version), encoding="utf-8"
    )
    with pytest.raises(script.ManifestError, match="signed for version"):
        release.make()
    assert release.outputs() == []


def test_refuses_a_signature_from_another_key(release: Release) -> None:
    name = release.installers["arm64"].name
    release.signatures["arm64"].write_text(
        sample_signature(name, key_id=OTHER_KEY_ID), encoding="utf-8"
    )
    with pytest.raises(script.ManifestError, match="another key"):
        release.make()
    assert release.outputs() == []


def test_accepts_both_minisign_signature_forms(release: Release) -> None:
    name = release.installers["x64"].name
    release.signatures["x64"].write_text(
        sample_signature(name, algorithm=b"Ed"), encoding="utf-8"
    )
    release.make()
    assert release.outputs() == ["SHA256SUMS.txt", "latest.json"]


@pytest.mark.parametrize("notes", ["", "   \n", "x" * 20_001])
def test_refuses_notes_that_are_empty_or_too_long(release: Release, notes: str) -> None:
    with pytest.raises(script.ManifestError, match="release notes"):
        release.make(notes=notes)


def test_refuses_a_config_without_the_public_key(release: Release) -> None:
    release.config.write_text(json.dumps({"plugins": {}}), encoding="utf-8")
    with pytest.raises(script.ManifestError, match="public key"):
        release.make()

    release.config.write_text(
        json.dumps({"plugins": {"updater": {"pubkey": encode(b"not a key")}}}),
        encoding="utf-8",
    )
    with pytest.raises(script.ManifestError, match="public key"):
        release.make()
    assert release.outputs() == []


def test_reads_the_public_key_of_this_repository() -> None:
    assert len(script.release_key_id(script.TAURI_CONFIG)) == 8


def test_command_line_writes_both_files(
    release: Release, capsys: pytest.CaptureFixture[str]
) -> None:
    notes = release.folder / "notes.md"
    notes.write_text(
        "# Deskmate 0.9.0\n\nSigned automatic updates.\n", encoding="utf-8"
    )

    code = script.main([*release.command_line(), "--notes-file", str(notes)])

    assert code == 0
    assert release.outputs() == ["SHA256SUMS.txt", "latest.json"]
    manifest = json.loads((release.out_dir / "latest.json").read_text(encoding="utf-8"))
    assert manifest["notes"] == "# Deskmate 0.9.0\n\nSigned automatic updates."
    assert re.fullmatch(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z", manifest["pub_date"])
    listed = capsys.readouterr().out
    for name in (
        "Deskmate_0.9.0_x64-setup.exe",
        "Deskmate_0.9.0_x64-setup.exe.sig",
        "Deskmate_0.9.0_arm64-setup.exe",
        "Deskmate_0.9.0_arm64-setup.exe.sig",
        "latest.json",
        "SHA256SUMS.txt",
    ):
        assert name in listed


def test_command_line_takes_signatures_from_another_place(release: Release) -> None:
    elsewhere = release.folder / "signatures"
    elsewhere.mkdir()
    moved = {}
    for architecture, signature in release.signatures.items():
        moved[architecture] = signature.rename(elsewhere / signature.name)

    arguments = [*release.command_line(), "--notes", "Signed automatic updates."]
    assert script.main(arguments) == 1
    assert release.outputs() == []

    arguments += ["--x64-sig", str(moved["x64"]), "--arm64-sig", str(moved["arm64"])]
    assert script.main(arguments) == 0
    assert release.outputs() == ["SHA256SUMS.txt", "latest.json"]


def test_command_line_reports_a_refusal(
    release: Release, capsys: pytest.CaptureFixture[str]
) -> None:
    arguments = [*release.command_line(), "--notes", "Signed automatic updates."]
    assert script.main(arguments) == 0

    assert script.main(arguments) == 1
    assert "already exists" in capsys.readouterr().err


def test_command_line_needs_notes(release: Release) -> None:
    with pytest.raises(SystemExit) as stopped:
        script.main(release.command_line())
    assert stopped.value.code == 2
    assert release.outputs() == []
