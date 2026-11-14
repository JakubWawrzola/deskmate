"""Write the update manifest and the checksums for a Deskmate release.

    python scripts/make_release_manifest.py --version 0.9.0 \\
        --x64 dist-installers/Deskmate_0.9.0_x64-setup.exe \\
        --arm64 dist-installers/Deskmate_0.9.0_arm64-setup.exe \\
        --notes-file docs/releases/RELEASE-0.9.0.md \\
        --out-dir dist-installers/0.9.0

Each installer needs the `.sig` file that `tauri build` wrote next to it; pass
`--x64-sig` or `--arm64-sig` when it is somewhere else. The script writes two
files into `--out-dir`, a folder for this release, and never replaces an
existing one:

- `latest.json`: what an installed Deskmate reads to find an update, with the
  download address and the signature for each processor architecture;
- `SHA256SUMS.txt`: the checksums of the two installers.

It checks what it can without the release key: the version, the file names,
that each signature was made for that file and that version, and that it
names the key the app trusts. It cannot check the signature itself (the
standard library has no Ed25519); the app does that before it installs.
"""

from __future__ import annotations

import argparse
import base64
import binascii
import hashlib
import json
import re
import sys
from datetime import UTC, datetime
from pathlib import Path

REPOSITORY_ROOT = Path(__file__).resolve().parent.parent
TAURI_CONFIG = REPOSITORY_ROOT / "src-tauri" / "tauri.conf.json"

# The app downloads an installer only from this address
# (src-tauri/src/updates/policy.rs).
RELEASE_DOWNLOADS = "https://github.com/JakubWawrzola/deskmate/releases/download"
# The app stops a download that grows past this size (MAX_INSTALLER_BYTES).
MAX_INSTALLER_BYTES = 64 * 1024 * 1024
MAX_NOTES_CHARS = 20_000
MANIFEST_NAME = "latest.json"
CHECKSUMS_NAME = "SHA256SUMS.txt"
# Platform key in the manifest -> architecture in the installer's file name.
PLATFORMS = {"windows-x86_64": "x64", "windows-aarch64": "arm64"}

VERSION_PATTERN = re.compile(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)")
WINDOWS_PROGRAM_MAGIC = b"MZ"
# minisign: two bytes for the algorithm, eight for the key id, then the key
# or the signature.
KEY_ID_BYTES = slice(2, 10)
PUBLIC_KEY_LENGTH = 2 + 8 + 32
SIGNATURE_LENGTH = 2 + 8 + 64
GLOBAL_SIGNATURE_LENGTH = 64
SIGNATURE_ALGORITHMS = (b"ED", b"Ed")
TRUSTED_COMMENT_PREFIX = "trusted comment: "


class ManifestError(Exception):
    """An input that must not go into a release."""


def checked_version(text: str) -> str:
    """A release version: `major.minor.patch` and nothing else.

    The app never installs a pre-release, so one is refused here as well.
    """
    if not VERSION_PATTERN.fullmatch(text):
        raise ManifestError(
            f"version {text!r} is not major.minor.patch, for example 0.9.0"
        )
    return text


def installer_name(version: str, architecture: str) -> str:
    return f"Deskmate_{version}_{architecture}-setup.exe"


def download_url(version: str, name: str) -> str:
    return f"{RELEASE_DOWNLOADS}/v{version}/{name}"


def decode_base64(text: str, what: str) -> bytes:
    try:
        return base64.b64decode(text, validate=True)
    except (binascii.Error, ValueError) as error:
        raise ManifestError(f"{what} is not valid base64") from error


def release_key_id(config: Path) -> bytes:
    """The id of the public key the app trusts, from tauri.conf.json."""
    try:
        settings = json.loads(config.read_text(encoding="utf-8"))
        encoded = settings["plugins"]["updater"]["pubkey"]
    except (OSError, ValueError, KeyError, TypeError) as error:
        raise ManifestError(
            f"cannot read the updater public key from {config}"
        ) from error
    if not isinstance(encoded, str):
        raise ManifestError(f"the updater public key in {config} is not a string")
    try:
        lines = decode_base64(encoded, "the updater public key").decode().splitlines()
    except UnicodeDecodeError as error:
        raise ManifestError("the updater public key is not text") from error
    if len(lines) != 2:
        raise ManifestError("the updater public key is not a minisign public key")
    key = decode_base64(lines[1], "the updater public key")
    if len(key) != PUBLIC_KEY_LENGTH:
        raise ManifestError("the updater public key has the wrong length")
    return key[KEY_ID_BYTES]


def installer_checksum(path: Path, expected_name: str) -> str:
    """The SHA-256 of an installer, after checking that it can be one."""
    if path.name != expected_name:
        raise ManifestError(f"{path.name}: expected a file named {expected_name}")
    if not path.is_file():
        raise ManifestError(f"{path}: no such file")
    size = path.stat().st_size
    if size == 0:
        raise ManifestError(f"{path.name} is empty")
    if size > MAX_INSTALLER_BYTES:
        raise ManifestError(
            f"{path.name} is larger than {MAX_INSTALLER_BYTES} bytes; "
            "the app would refuse to download it"
        )
    digest = hashlib.sha256()
    with path.open("rb") as installer:
        if installer.read(len(WINDOWS_PROGRAM_MAGIC)) != WINDOWS_PROGRAM_MAGIC:
            raise ManifestError(f"{path.name} is not a Windows program")
        installer.seek(0)
        for block in iter(lambda: installer.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def checked_signature(path: Path, name: str, version: str, key_id: bytes) -> str:
    """The content of a `.sig` file, for the manifest.

    The signature has to be made for the file `name` and for `version`, by the
    key the app trusts. The app requires all three and would refuse the update
    otherwise.
    """
    if not path.is_file():
        raise ManifestError(
            f"{path}: no such file; `tauri build` writes it next to {name}"
        )
    try:
        encoded = path.read_text(encoding="utf-8").strip()
        lines = (
            decode_base64(encoded, f"the signature {path.name}").decode().splitlines()
        )
    except (OSError, UnicodeDecodeError) as error:
        raise ManifestError(f"{path.name} is not a signature file") from error
    if len(lines) != 4 or not lines[2].startswith(TRUSTED_COMMENT_PREFIX):
        raise ManifestError(f"{path.name} is not a minisign signature")
    signature = decode_base64(lines[1], f"the signature {path.name}")
    global_signature = decode_base64(lines[3], f"the signature {path.name}")
    if (
        len(signature) != SIGNATURE_LENGTH
        or signature[:2] not in SIGNATURE_ALGORITHMS
        or len(global_signature) != GLOBAL_SIGNATURE_LENGTH
    ):
        raise ManifestError(f"{path.name} is not a minisign signature")
    if signature[KEY_ID_BYTES] != key_id:
        raise ManifestError(
            f"{path.name} was made with another key than the one in tauri.conf.json"
        )
    comment = lines[2].removeprefix(TRUSTED_COMMENT_PREFIX)
    fields = dict(field.split(":", 1) for field in comment.split("\t") if ":" in field)
    if fields.get("file") != name:
        raise ManifestError(f"{path.name} signs {fields.get('file')!r}, not {name}")
    if fields.get("version") != version:
        raise ManifestError(
            f"{path.name} is signed for version {fields.get('version')!r}, not {version}"
        )
    return encoded


def checked_notes(notes: str) -> str:
    notes = notes.strip()
    if not notes:
        raise ManifestError("the release notes are empty")
    if len(notes) > MAX_NOTES_CHARS:
        raise ManifestError(
            f"the release notes are longer than {MAX_NOTES_CHARS} characters"
        )
    return notes


def write_new(path: Path, text: str) -> None:
    """Writes a file that must not exist yet."""
    try:
        with path.open("x", encoding="utf-8", newline="\n") as output:
            output.write(text)
    except FileExistsError as error:
        raise ManifestError(f"{path} already exists; it is not replaced") from error


def make_release(
    version: str,
    installers: dict[str, Path],
    signatures: dict[str, Path],
    notes: str,
    out_dir: Path,
    config: Path = TAURI_CONFIG,
    published: datetime | None = None,
) -> tuple[Path, Path]:
    """Checks the inputs and writes `latest.json` and `SHA256SUMS.txt`.

    `installers` and `signatures` are keyed by architecture: "x64", "arm64".
    """
    version = checked_version(version)
    notes = checked_notes(notes)
    key_id = release_key_id(config)
    manifest_path = out_dir / MANIFEST_NAME
    checksums_path = out_dir / CHECKSUMS_NAME
    if out_dir.exists() and not out_dir.is_dir():
        raise ManifestError(f"{out_dir} is not a folder")
    if not out_dir.is_dir() and not out_dir.parent.is_dir():
        raise ManifestError(f"{out_dir.parent}: no such folder")
    for output in (manifest_path, checksums_path):
        if output.exists():
            raise ManifestError(f"{output} already exists; it is not replaced")

    platforms = {}
    checksums = {}
    for platform, architecture in PLATFORMS.items():
        name = installer_name(version, architecture)
        checksums[name] = installer_checksum(installers[architecture], name)
        platforms[platform] = {
            "signature": checked_signature(
                signatures[architecture], name, version, key_id
            ),
            "url": download_url(version, name),
        }

    out_dir.mkdir(exist_ok=True)
    published = published or datetime.now(UTC)
    manifest = {
        "version": version,
        "notes": notes,
        "pub_date": published.astimezone(UTC).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "platforms": platforms,
    }
    write_new(manifest_path, json.dumps(manifest, indent=2, ensure_ascii=False) + "\n")
    write_new(
        checksums_path,
        "".join(
            f"{checksum}  {name}\n" for name, checksum in sorted(checksums.items())
        ),
    )
    return manifest_path, checksums_path


def signature_path(given: Path | None, installer: Path) -> Path:
    return given if given is not None else installer.with_name(installer.name + ".sig")


def main(arguments: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Write latest.json and SHA256SUMS.txt for a Deskmate release."
    )
    parser.add_argument("--version", required=True, help="for example 0.9.0")
    parser.add_argument("--x64", required=True, type=Path, help="the x64 installer")
    parser.add_argument("--arm64", required=True, type=Path, help="the ARM64 installer")
    parser.add_argument(
        "--x64-sig", type=Path, help="default: the x64 installer + .sig"
    )
    parser.add_argument(
        "--arm64-sig", type=Path, help="default: the ARM64 installer + .sig"
    )
    notes = parser.add_mutually_exclusive_group(required=True)
    notes.add_argument("--notes", help="what changed, as text")
    notes.add_argument("--notes-file", type=Path, help="what changed, as a UTF-8 file")
    parser.add_argument(
        "--out-dir", required=True, type=Path, help="where the two files go"
    )
    parser.add_argument(
        "--config",
        type=Path,
        default=TAURI_CONFIG,
        help="tauri.conf.json with the updater public key (default: this repository's)",
    )
    args = parser.parse_args(arguments)

    try:
        if args.notes_file is not None:
            try:
                notes_text = args.notes_file.read_text(encoding="utf-8")
            except (OSError, UnicodeDecodeError) as error:
                raise ManifestError(f"cannot read {args.notes_file}") from error
        else:
            notes_text = args.notes
        manifest_path, checksums_path = make_release(
            version=args.version,
            installers={"x64": args.x64, "arm64": args.arm64},
            signatures={
                "x64": signature_path(args.x64_sig, args.x64),
                "arm64": signature_path(args.arm64_sig, args.arm64),
            },
            notes=notes_text,
            out_dir=args.out_dir,
            config=args.config,
        )
    except ManifestError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1

    print(f"wrote {manifest_path}")
    print(f"wrote {checksums_path}")
    print("upload to the GitHub release:")
    for path in (
        args.x64,
        signature_path(args.x64_sig, args.x64),
        args.arm64,
        signature_path(args.arm64_sig, args.arm64),
        manifest_path,
        checksums_path,
    ):
        print(f"  {path.name}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
