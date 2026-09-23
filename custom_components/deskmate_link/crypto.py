"""Kryptografia Deskmate Link v1.

PSK 32 B -> handshake HMAC-SHA256 -> klucze sesji HKDF-SHA256 ->
ramki AES-256-GCM z licznikiem (anty-replay). Szczegoly: docs/DESKMATE-LINK.md.
Uzywa pakietu `cryptography` (jest w srodowisku HA Core).
"""
from __future__ import annotations

import base64
import hmac as _hmac
import json
import secrets
from hashlib import sha256

from cryptography.hazmat.primitives import hashes
from cryptography.hazmat.primitives.ciphers.aead import AESGCM, ChaCha20Poly1305
from cryptography.hazmat.primitives.kdf.hkdf import HKDF


def gen_psk() -> str:
    """Nowy klucz parowania (base64, 32 B)."""
    return base64.b64encode(secrets.token_bytes(32)).decode()


def gen_nonce16() -> str:
    return base64.b64encode(secrets.token_bytes(16)).decode()


def _b64d(value: str) -> bytes:
    return base64.b64decode(value.encode())


def hs_mac(psk_b64: str, *parts: str) -> str:
    """MAC handshake'u: HMAC-SHA256(psk, "|".join(parts))."""
    mac = _hmac.new(_b64d(psk_b64), "|".join(parts).encode(), sha256).digest()
    return base64.b64encode(mac).decode()


def hs_mac_ok(psk_b64: str, mac_b64: str, *parts: str) -> bool:
    try:
        expected = _b64d(hs_mac(psk_b64, *parts))
        return _hmac.compare_digest(expected, _b64d(mac_b64))
    except Exception:  # zly base64 itd. = nieprawidlowy MAC
        return False


def derive_session_keys(psk_b64: str, cn_b64: str, sn_b64: str) -> tuple[bytes, bytes]:
    """Zwraca (k_c2s, k_s2c)."""
    salt = _b64d(cn_b64) + _b64d(sn_b64)
    keys = []
    for info in (b"dml1 c2s", b"dml1 s2c"):
        keys.append(
            HKDF(algorithm=hashes.SHA256(), length=32, salt=salt, info=info).derive(
                _b64d(psk_b64)
            )
        )
    return keys[0], keys[1]


def derive_cascade_keys(psk_b64: str, cn_b64: str, sn_b64: str) -> tuple[bytes, bytes]:
    """Klucze drugiej warstwy (ChaCha20-Poly1305), z osobnego klucza kaskady."""
    salt = _b64d(cn_b64) + _b64d(sn_b64)
    keys = []
    for info in (b"dml1 cascade c2s", b"dml1 cascade s2c"):
        keys.append(
            HKDF(algorithm=hashes.SHA256(), length=32, salt=salt, info=info).derive(
                _b64d(psk_b64)
            )
        )
    return keys[0], keys[1]


class FrameCodec:
    """Szyfrowanie/deszyfrowanie ramek jednego kierunku.

    Z kluczem kaskady ramka jest szyfrowana dwa razy, roznymi szyframi i roznymi
    kluczami: AES-256-GCM w srodku, ChaCha20-Poly1305 na zewnatrz. Zlamanie
    jednego z nich nie wystarcza, zeby odczytac ruch.
    """

    def __init__(
        self,
        key: bytes,
        direction: bytes,
        node: str,
        dir_name: str,
        cascade_key: bytes | None = None,
    ) -> None:
        self._aead = AESGCM(key)
        self._cascade = ChaCha20Poly1305(cascade_key) if cascade_key else None
        self._direction = direction
        self._aad = f"{node}|{dir_name}".encode()
        self._cascade_aad = f"{node}|{dir_name}|cascade".encode()
        self.counter = 0  # ostatni uzyty (nadawca) / ostatni przyjety (odbiorca)

    def _nonce(self, counter: int) -> bytes:
        return self._direction + counter.to_bytes(8, "big")

    def encrypt(self, payload: dict) -> dict:
        self.counter += 1
        nonce = self._nonce(self.counter)
        ct = self._aead.encrypt(nonce, json.dumps(payload).encode(), self._aad)
        if self._cascade is not None:
            ct = self._cascade.encrypt(nonce, ct, self._cascade_aad)
        return {"t": "e", "n": self.counter, "p": base64.b64encode(ct).decode()}

    def decrypt(self, frame: dict) -> dict:
        """Podnosi ValueError przy replayu/uszkodzeniu - rozlacz sesje."""
        n = frame.get("n")
        if not isinstance(n, int) or n <= self.counter:
            raise ValueError("replay or out-of-order frame")
        nonce = self._nonce(n)
        ct = _b64d(frame["p"])
        if self._cascade is not None:
            ct = self._cascade.decrypt(nonce, ct, self._cascade_aad)
        pt = self._aead.decrypt(nonce, ct, self._aad)
        self.counter = n
        return json.loads(pt.decode())
