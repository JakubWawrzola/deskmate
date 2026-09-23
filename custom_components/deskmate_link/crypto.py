"""Kryptografia Deskmate Link v1 i v2.

v1: PSK 32 B -> handshake HMAC-SHA256 -> klucze sesji HKDF-SHA256(PSK, cn|sn).
    Bez forward secrecy: kto zdobedzie PSK, odszyfruje nagrany ruch.
v2: to samo uwierzytelnienie PSK, ale klucze sesji pochodza z efemerycznego
    X25519 zmieszanego z PSK, a MAC obejmuje caly transkrypt handshake'u
    (wersja, node, nonce, czas, klucze publiczne, flaga kaskady). Wyciek PSK
    nie odslania juz nagranych sesji.

Pola do MAC i KDF sa kodowane z prefiksem dlugosci (`enc`), wiec zadne
sklejenie pol nie da tego samego ciagu bajtow co inne.
Uzywa pakietu `cryptography` (jest w srodowisku HA Core).
"""
from __future__ import annotations

import base64
import hmac as _hmac
import json
import secrets
from hashlib import sha256

from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric.x25519 import (
    X25519PrivateKey,
    X25519PublicKey,
)
from cryptography.hazmat.primitives.ciphers.aead import AESGCM, ChaCha20Poly1305
from cryptography.hazmat.primitives.kdf.hkdf import HKDF


def gen_psk() -> str:
    """Nowy klucz parowania (base64, 32 B)."""
    return base64.b64encode(secrets.token_bytes(32)).decode()


def gen_nonce16() -> str:
    return base64.b64encode(secrets.token_bytes(16)).decode()


def _b64d(value: str) -> bytes:
    return base64.b64decode(value.encode(), validate=True)


def b64d_exact(value: object, size: int) -> bytes | None:
    """Base64 z niezaufanej ramki o dokladnie `size` bajtach albo None."""
    if not isinstance(value, str):
        return None
    try:
        raw = _b64d(value)
    except (ValueError, TypeError):
        return None
    return raw if len(raw) == size else None


# ── v1 (zgodnosc wsteczna) ───────────────────────────────────────


def hs_mac(psk_b64: str, *parts: str) -> str:
    """MAC handshake'u v1: HMAC-SHA256(psk, "|".join(parts))."""
    mac = _hmac.new(_b64d(psk_b64), "|".join(parts).encode(), sha256).digest()
    return base64.b64encode(mac).decode()


def hs_mac_ok(psk_b64: str, mac_b64: str, *parts: str) -> bool:
    try:
        expected = _b64d(hs_mac(psk_b64, *parts))
        return _hmac.compare_digest(expected, _b64d(mac_b64))
    except Exception:  # zly base64 itd. = nieprawidlowy MAC
        return False


def _hkdf_pair(
    ikm: bytes, salt: bytes, info_c2s: bytes, info_s2c: bytes
) -> tuple[bytes, bytes]:
    return tuple(  # type: ignore[return-value]
        HKDF(algorithm=hashes.SHA256(), length=32, salt=salt, info=info).derive(ikm)
        for info in (info_c2s, info_s2c)
    )


def derive_session_keys(psk_b64: str, cn_b64: str, sn_b64: str) -> tuple[bytes, bytes]:
    """v1: zwraca (k_c2s, k_s2c)."""
    salt = _b64d(cn_b64) + _b64d(sn_b64)
    return _hkdf_pair(_b64d(psk_b64), salt, b"dml1 c2s", b"dml1 s2c")


def derive_cascade_keys(psk_b64: str, cn_b64: str, sn_b64: str) -> tuple[bytes, bytes]:
    """v1: klucze drugiej warstwy (ChaCha20-Poly1305), z osobnego klucza kaskady."""
    salt = _b64d(cn_b64) + _b64d(sn_b64)
    return _hkdf_pair(
        _b64d(psk_b64), salt, b"dml1 cascade c2s", b"dml1 cascade s2c"
    )


# ── v2 ───────────────────────────────────────────────────────────


def enc(*parts: bytes) -> bytes:
    """Kodowanie kanoniczne: kazde pole poprzedzone 4-bajtowa dlugoscia (BE)."""
    out = bytearray()
    for part in parts:
        out += len(part).to_bytes(4, "big")
        out += part
    return bytes(out)


def v2_hello_bytes(
    node: str, cn: bytes, ts: int, epk: bytes, cascade: bool
) -> bytes:
    return enc(
        b"dml2 hello",
        b"2",
        node.encode(),
        cn,
        str(int(ts)).encode(),
        epk,
        b"1" if cascade else b"0",
    )


def v2_welcome_bytes(hello: bytes, sn: bytes, ts: int, epk: bytes) -> bytes:
    """Welcome wiaze sie z hash'em calego hello - podmiana dowolnego pola
    hello (np. flagi kaskady) psuje MAC odpowiedzi."""
    return enc(b"dml2 welcome", sha256(hello).digest(), sn, str(int(ts)).encode(), epk)


def v2_mac(psk_b64: str, data: bytes) -> str:
    return base64.b64encode(_hmac.new(_b64d(psk_b64), data, sha256).digest()).decode()


def v2_mac_ok(psk_b64: str, mac_b64: object, data: bytes) -> bool:
    supplied = b64d_exact(mac_b64, 32)
    if supplied is None:
        return False
    expected = _hmac.new(_b64d(psk_b64), data, sha256).digest()
    return _hmac.compare_digest(expected, supplied)


def _v2_salt(hello: bytes, welcome: bytes) -> bytes:
    return sha256(enc(hello, welcome)).digest()


def v2_session_keys(
    psk_b64: str, shared: bytes, hello: bytes, welcome: bytes
) -> tuple[bytes, bytes]:
    """Klucze sesji v2: HKDF(ikm = DH || PSK, salt = hash transkryptu)."""
    return _hkdf_pair(
        shared + _b64d(psk_b64), _v2_salt(hello, welcome), b"dml2 c2s", b"dml2 s2c"
    )


def v2_cascade_keys(
    cascade_b64: str, shared: bytes, hello: bytes, welcome: bytes
) -> tuple[bytes, bytes]:
    return _hkdf_pair(
        shared + _b64d(cascade_b64),
        _v2_salt(hello, welcome),
        b"dml2 cascade c2s",
        b"dml2 cascade s2c",
    )


class EphemeralKey:
    """Jednorazowa para X25519 na jeden handshake."""

    def __init__(self) -> None:
        self._private = X25519PrivateKey.generate()
        self.public = self._private.public_key().public_bytes(
            serialization.Encoding.Raw, serialization.PublicFormat.Raw
        )

    def exchange(self, peer: bytes) -> bytes | None:
        """Wspolny sekret albo None dla punktu malego rzedu (same zera)."""
        try:
            shared = self._private.exchange(X25519PublicKey.from_public_bytes(peer))
        except ValueError:
            return None
        if not any(shared):
            return None
        return shared


# ── ramki ────────────────────────────────────────────────────────


class FrameCodec:
    """Szyfrowanie/deszyfrowanie ramek jednego kierunku.

    Z kluczem kaskady ramka jest szyfrowana dwa razy, roznymi szyframi i roznymi
    kluczami: AES-256-GCM w srodku, ChaCha20-Poly1305 na zewnatrz.
    """

    def __init__(
        self,
        key: bytes,
        direction: bytes,
        node: str,
        dir_name: str,
        cascade_key: bytes | None = None,
        version: int = 1,
    ) -> None:
        self._aead = AESGCM(key)
        self._cascade = ChaCha20Poly1305(cascade_key) if cascade_key else None
        self._direction = direction
        if version >= 2:
            self._aad = enc(b"dml2", node.encode(), dir_name.encode())
            self._cascade_aad = enc(b"dml2 cascade", node.encode(), dir_name.encode())
        else:
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
        if not isinstance(n, int) or isinstance(n, bool) or n <= self.counter:
            raise ValueError("replay or out-of-order frame")
        if n >= 2**64:
            raise ValueError("frame counter out of range")
        payload = frame.get("p")
        if not isinstance(payload, str):
            raise ValueError("missing frame payload")
        nonce = self._nonce(n)
        try:
            ct = _b64d(payload)
            if self._cascade is not None:
                ct = self._cascade.decrypt(nonce, ct, self._cascade_aad)
            pt = self._aead.decrypt(nonce, ct, self._aad)
        except Exception as err:  # InvalidTag, zly base64
            raise ValueError("frame authentication failed") from err
        self.counter = n
        return json.loads(pt.decode())
