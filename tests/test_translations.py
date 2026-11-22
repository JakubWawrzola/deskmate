"""Every translation of the integration says what `strings.json` says.

`strings.json` is the source, in English. A translation must have exactly its
keys and keep its placeholders, or Home Assistant shows a raw key or a text
with a hole in it.
"""

from __future__ import annotations

import json
import re
from collections.abc import Iterator
from pathlib import Path
from typing import Any

import pytest

COMPONENT = Path(__file__).parent.parent / "custom_components" / "deskmate_link"
TRANSLATIONS = sorted((COMPONENT / "translations").glob("*.json"))
LANGUAGES = {"de", "en", "es", "fr", "pl", "pt-BR", "zh-Hans"}
# `{code}`, and the code-like words a translation must not translate,
# because the user types them or reads them elsewhere. Product names are not
# in the list: a language may decline them or use one more often.
_PLACEHOLDER = re.compile(r"\{[a-z_]+\}")
_LITERALS = (
    "allowlist_external_dirs",
    "deskmate_link_notify_action",
    "/config/deskmate_tv",
    "DeskmateTV-X.Y.Z.apk",
    "AES-256-GCM",
    "ChaCha20-Poly1305",
    "X25519",
    "SHA-256",
)


def texts(tree: dict[str, Any], prefix: str = "") -> Iterator[tuple[str, str]]:
    for key, value in tree.items():
        if isinstance(value, dict):
            yield from texts(value, f"{prefix}{key}.")
        else:
            yield f"{prefix}{key}", value


def load(path: Path) -> dict[str, str]:
    return dict(texts(json.loads(path.read_text(encoding="utf-8"))))


SOURCE = load(COMPONENT / "strings.json")


def test_every_language_of_the_project_is_there() -> None:
    assert {path.stem for path in TRANSLATIONS} == LANGUAGES


@pytest.mark.parametrize("path", TRANSLATIONS, ids=lambda path: path.stem)
def test_translation_has_exactly_the_keys_of_the_source(path: Path) -> None:
    translated = load(path)

    assert sorted(translated) == sorted(SOURCE)
    assert all(text.strip() for text in translated.values())


@pytest.mark.parametrize("path", TRANSLATIONS, ids=lambda path: path.stem)
def test_translation_keeps_placeholders_and_literal_names(path: Path) -> None:
    translated = load(path)

    for key, source in SOURCE.items():
        text = translated[key]
        assert sorted(_PLACEHOLDER.findall(text)) == sorted(
            _PLACEHOLDER.findall(source)
        ), key
        for literal in _LITERALS:
            assert text.count(literal) == source.count(literal), (key, literal)
        # Paragraph breaks carry the layout of the config flow's dialogs.
        assert text.count("\n\n") == source.count("\n\n"), key


def test_english_is_the_source_itself() -> None:
    assert load(COMPONENT / "translations" / "en.json") == SOURCE
