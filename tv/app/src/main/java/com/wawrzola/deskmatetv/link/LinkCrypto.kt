package com.wawrzola.deskmatetv.link

import java.io.ByteArrayOutputStream
import java.security.MessageDigest
import java.util.Base64
import javax.crypto.Cipher
import javax.crypto.Mac
import javax.crypto.spec.GCMParameterSpec
import javax.crypto.spec.IvParameterSpec
import javax.crypto.spec.SecretKeySpec

/**
 * Deskmate Link v2 - lustro `deskmate_link/crypto.py` z HA i `link.rs` z Deskmate.
 *
 * Czysty JVM (bez API Androida), zeby testy wektorowe szly zwyklym JUnitem.
 * Kazda zmiana tu musi przejsc `./gradlew test` z wektorami z Pythona.
 */
object LinkCrypto {
    const val PROTO_V2 = 2
    val DIR_C2S = byteArrayOf(1, 0, 0, 0)
    val DIR_S2C = byteArrayOf(2, 0, 0, 0)

    private val b64e = Base64.getEncoder()
    private val b64d = Base64.getDecoder()

    fun b64(bytes: ByteArray): String = b64e.encodeToString(bytes)

    /** Base64 z niezaufanego zrodla o dokladnie [size] bajtach albo null. */
    fun b64Exact(value: String?, size: Int): ByteArray? {
        if (value == null) return null
        val raw = try {
            b64d.decode(value)
        } catch (_: IllegalArgumentException) {
            return null
        }
        return if (raw.size == size) raw else null
    }

    fun b64Decode(value: String): ByteArray = b64d.decode(value)

    /** Kodowanie kanoniczne: kazde pole z 4-bajtowa dlugoscia (big-endian). */
    fun enc(vararg parts: ByteArray): ByteArray {
        val out = ByteArrayOutputStream()
        for (part in parts) {
            out.write((part.size ushr 24) and 0xff)
            out.write((part.size ushr 16) and 0xff)
            out.write((part.size ushr 8) and 0xff)
            out.write(part.size and 0xff)
            out.write(part)
        }
        return out.toByteArray()
    }

    fun sha256(data: ByteArray): ByteArray = MessageDigest.getInstance("SHA-256").digest(data)

    fun hmac(key: ByteArray, data: ByteArray): ByteArray {
        val mac = Mac.getInstance("HmacSHA256")
        mac.init(SecretKeySpec(key, "HmacSHA256"))
        return mac.doFinal(data)
    }

    /** HKDF-SHA256 (RFC 5869) z dlugoscia 32 B - tyle uzywa Link. */
    fun hkdf32(ikm: ByteArray, salt: ByteArray, info: ByteArray): ByteArray {
        val prk = hmac(salt, ikm)
        return hmac(prk, info + byteArrayOf(1))
    }

    fun helloBytes(node: String, cn: ByteArray, ts: Long, epk: ByteArray, cascade: Boolean): ByteArray =
        enc(
            "dml2 hello".toByteArray(),
            "2".toByteArray(),
            node.toByteArray(),
            cn,
            ts.toString().toByteArray(),
            epk,
            (if (cascade) "1" else "0").toByteArray(),
        )

    fun welcomeBytes(hello: ByteArray, sn: ByteArray, ts: Long, epk: ByteArray): ByteArray =
        enc("dml2 welcome".toByteArray(), sha256(hello), sn, ts.toString().toByteArray(), epk)

    fun macOk(key: ByteArray, data: ByteArray, supplied: ByteArray?): Boolean {
        if (supplied == null || supplied.size != 32) return false
        return MessageDigest.isEqual(hmac(key, data), supplied)
    }

    private fun salt(hello: ByteArray, welcome: ByteArray): ByteArray = sha256(enc(hello, welcome))

    /** Klucze sesji v2: HKDF(ikm = DH || PSK, salt = hash transkryptu) -> (c2s, s2c). */
    fun sessionKeys(psk: ByteArray, shared: ByteArray, hello: ByteArray, welcome: ByteArray): Pair<ByteArray, ByteArray> {
        val s = salt(hello, welcome)
        val ikm = shared + psk
        return hkdf32(ikm, s, "dml2 c2s".toByteArray()) to hkdf32(ikm, s, "dml2 s2c".toByteArray())
    }

    fun cascadeKeys(cascade: ByteArray, shared: ByteArray, hello: ByteArray, welcome: ByteArray): Pair<ByteArray, ByteArray> {
        val s = salt(hello, welcome)
        val ikm = shared + cascade
        return hkdf32(ikm, s, "dml2 cascade c2s".toByteArray()) to
            hkdf32(ikm, s, "dml2 cascade s2c".toByteArray())
    }
}

/**
 * Szyfrowanie ramek jednego kierunku (v2). Z kluczem kaskady ramka jest
 * szyfrowana dwa razy: AES-256-GCM w srodku, ChaCha20-Poly1305 na zewnatrz.
 */
class FrameCodec(
    private val key: ByteArray,
    private val direction: ByteArray,
    node: String,
    dirName: String,
    private val cascadeKey: ByteArray? = null,
) {
    private val aad = LinkCrypto.enc("dml2".toByteArray(), node.toByteArray(), dirName.toByteArray())
    private val cascadeAad =
        LinkCrypto.enc("dml2 cascade".toByteArray(), node.toByteArray(), dirName.toByteArray())

    /** Ostatni uzyty (nadawca) albo ostatni przyjety (odbiorca) licznik. */
    var counter: Long = 0
        private set

    private fun nonce(n: Long): ByteArray {
        val out = ByteArray(12)
        System.arraycopy(direction, 0, out, 0, 4)
        for (i in 0 until 8) out[4 + i] = ((n ushr (56 - 8 * i)) and 0xff).toByte()
        return out
    }

    /** Zwraca (licznik, szyfrogram base64) dla tekstu JSON wiadomosci. */
    fun encrypt(plaintext: String): Pair<Long, String> {
        counter += 1
        val n = counter
        val iv = nonce(n)
        var ct = aesGcm(Cipher.ENCRYPT_MODE, iv, plaintext.toByteArray(Charsets.UTF_8))
        if (cascadeKey != null) ct = chacha(Cipher.ENCRYPT_MODE, iv, ct)
        return n to LinkCrypto.b64(ct)
    }

    /** Ramka t:"e" -> tekst JSON. Rzuca IllegalStateException przy replayu/uszkodzeniu. */
    fun decrypt(n: Long, payloadB64: String): String {
        if (n <= counter) throw IllegalStateException("replay or out-of-order frame")
        val iv = nonce(n)
        val pt = try {
            var ct = LinkCrypto.b64Decode(payloadB64)
            if (cascadeKey != null) ct = chacha(Cipher.DECRYPT_MODE, iv, ct)
            aesGcm(Cipher.DECRYPT_MODE, iv, ct)
        } catch (e: Exception) {
            throw IllegalStateException("frame authentication failed", e)
        }
        counter = n
        return String(pt, Charsets.UTF_8)
    }

    private fun aesGcm(mode: Int, iv: ByteArray, data: ByteArray): ByteArray {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(mode, SecretKeySpec(key, "AES"), GCMParameterSpec(128, iv))
        cipher.updateAAD(aad)
        return cipher.doFinal(data)
    }

    private fun chacha(mode: Int, iv: ByteArray, data: ByteArray): ByteArray {
        val cipher = chachaCipher()
        cipher.init(mode, SecretKeySpec(cascadeKey, "ChaCha20"), IvParameterSpec(iv))
        cipher.updateAAD(cascadeAad)
        return cipher.doFinal(data)
    }

    private fun chachaCipher(): Cipher =
        try {
            Cipher.getInstance("ChaCha20-Poly1305") // JDK 11+
        } catch (_: Exception) {
            Cipher.getInstance("ChaCha20/Poly1305/NoPadding") // Conscrypt (Android 9+)
        }
}

/** Wynik udanego handshake'u po stronie klienta. */
class LinkSession(val tx: FrameCodec, val rx: FrameCodec)

/** Stan handshake'u klienta: buduje hello, weryfikuje welcome, liczy klucze. */
class ClientHandshake(
    private val psk: ByteArray,
    private val node: String,
    private val cascade: ByteArray? = null,
    private val nowSeconds: () -> Long = { System.currentTimeMillis() / 1000 },
    cn: ByteArray? = null,
    ephemeralPrivate: ByteArray? = null,
) {
    private val cn: ByteArray = cn ?: ByteArray(16).also { java.security.SecureRandom().nextBytes(it) }
    private var priv: ByteArray? = ephemeralPrivate ?: X25519.generatePrivate()
    private val epk: ByteArray = X25519.publicKey(priv!!)
    private val ts: Long = nowSeconds()
    val helloData: ByteArray = LinkCrypto.helloBytes(node, this.cn, ts, epk, cascade != null)

    /** Ramka hello jako JSON (recznie - bez org.json, zeby dzialalo w testach JVM). */
    fun helloJson(): String =
        "{\"t\":\"hello\",\"v\":2,\"node\":\"" + node + "\",\"cn\":\"" + LinkCrypto.b64(cn) +
            "\",\"ts\":" + ts + ",\"epk\":\"" + LinkCrypto.b64(epk) + "\",\"casc\":" +
            (cascade != null) + ",\"mac\":\"" + LinkCrypto.b64(LinkCrypto.hmac(psk, helloData)) + "\"}"

    class HandshakeError(val reason: String, message: String) : Exception(message)

    /**
     * Weryfikuje welcome. [v], [snB64], [ts], [epkB64], [macB64] to pola ramki.
     * Rzuca [HandshakeError]: "version", "clock", "auth".
     */
    fun finish(v: Int, snB64: String?, ts: Long, epkB64: String?, macB64: String?, maxSkew: Long = 90): LinkSession {
        if (v != LinkCrypto.PROTO_V2) throw HandshakeError("version", "serwer nie mowi Link v2")
        val sn = LinkCrypto.b64Exact(snB64, 16) ?: throw HandshakeError("auth", "zly nonce serwera")
        val serverEpk = LinkCrypto.b64Exact(epkB64, 32) ?: throw HandshakeError("auth", "zly klucz serwera")
        if (kotlin.math.abs(nowSeconds() - ts) > maxSkew) {
            throw HandshakeError("clock", "zegar TV rozjechany wzgledem HA")
        }
        val welcomeData = LinkCrypto.welcomeBytes(helloData, sn, ts, serverEpk)
        if (!LinkCrypto.macOk(psk, welcomeData, LinkCrypto.b64Exact(macB64, 32))) {
            throw HandshakeError("auth", "podpis welcome sie nie zgadza (zly klucz?)")
        }
        val key = priv ?: throw HandshakeError("auth", "handshake juz uzyty")
        val shared = X25519.sharedSecret(key, serverEpk) ?: throw HandshakeError("auth", "klucz serwera malego rzedu")
        key.fill(0)
        priv = null
        val (c2s, s2c) = LinkCrypto.sessionKeys(psk, shared, helloData, welcomeData)
        var cascC2s: ByteArray? = null
        var cascS2c: ByteArray? = null
        if (cascade != null) {
            val pair = LinkCrypto.cascadeKeys(cascade, shared, helloData, welcomeData)
            cascC2s = pair.first
            cascS2c = pair.second
        }
        shared.fill(0)
        return LinkSession(
            tx = FrameCodec(c2s, LinkCrypto.DIR_C2S, node, "c2s", cascC2s),
            rx = FrameCodec(s2c, LinkCrypto.DIR_S2C, node, "s2c", cascS2c),
        )
    }
}
