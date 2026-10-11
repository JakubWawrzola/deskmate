package com.wawrzola.deskmatetv.link

import org.json.JSONObject
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test

/**
 * Zgodnosc z HA: wektory generuje tools/gen_vectors.py z prawdziwego
 * deskmate_link/crypto.py. Rozjazd = TV nie dogada sie z HA.
 */
class LinkCryptoTest {
    private val v: JSONObject by lazy {
        val text = javaClass.classLoader!!.getResource("link_vectors.json")!!.readText()
        JSONObject(text)
    }

    private fun b64(key: String) = LinkCrypto.b64Decode(v.getString(key))
    private fun hex(s: String): ByteArray = ByteArray(s.length / 2) { s.substring(it * 2, it * 2 + 2).toInt(16).toByte() }
    private fun ByteArray.hex() = joinToString("") { "%02x".format(it) }

    @Test
    fun rfc7748Vectors() {
        // RFC 7748, sekcja 5.2 (pierwszy wektor) i 6.1 (Diffie-Hellman)
        val scalar = hex("a546e36bf0527c9d3b16154b82465edd62144c0ac1fc5a18506a2244ba449ac4")
        val u = hex("e6db6867583030db3594c1a424b15f7c726624ec26b3353b10a903a6d0ab1c4c")
        assertEquals(
            "c3da55379de9c6908e94ea4df28d084f32eccf03491c71f754b4075577a28552",
            X25519.scalarMult(scalar, u).hex(),
        )
        val alicePriv = hex("77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a")
        val bobPriv = hex("5dab087e624a8a4b79e17f8b83800ee66f3bb1292618b6fd1c2f8b27ff88e0eb")
        assertEquals(
            "8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a",
            X25519.publicKey(alicePriv).hex(),
        )
        val bobPub = X25519.publicKey(bobPriv)
        assertEquals("de9edb7d7b7dc1b4d35b61c2ece435373f8343c85b78674dadfc7e146f882b4f", bobPub.hex())
        assertEquals(
            "4a5d9d5ba4ce2de1728e3bf480350f25e07e21c947d19e3376f09b3c1e161742",
            X25519.sharedSecret(alicePriv, bobPub)!!.hex(),
        )
        assertNull(X25519.sharedSecret(alicePriv, ByteArray(32)))
    }

    @Test
    fun x25519MatchesPython() {
        assertArrayEquals(b64("client_epk"), X25519.publicKey(b64("client_priv")))
        assertArrayEquals(b64("server_epk"), X25519.publicKey(b64("server_priv")))
        assertArrayEquals(b64("shared"), X25519.sharedSecret(b64("client_priv"), b64("server_epk")))
    }

    @Test
    fun handshakeAndFramesPlain() = checkVariant("plain", null)

    @Test
    fun handshakeAndFramesCascade() = checkVariant("casc", b64("cascade"))

    private fun checkVariant(tag: String, cascade: ByteArray?) {
        val t = v.getJSONObject(tag)
        val psk = b64("psk")
        val node = v.getString("node")
        val tsC = v.getLong("ts_c")
        val tsS = v.getLong("ts_s")
        val hs = ClientHandshake(
            psk = psk,
            node = node,
            cascade = cascade,
            nowSeconds = { tsC },
            cn = b64("cn"),
            ephemeralPrivate = b64("client_priv"),
        )
        assertEquals(t.getString("hello_bytes"), hs.helloData.hex())
        val hello = JSONObject(hs.helloJson())
        assertEquals(t.getString("hello_mac"), hello.getString("mac"))
        assertEquals(2, hello.getInt("v"))
        assertEquals(cascade != null, hello.getBoolean("casc"))
        assertEquals(tsC, hello.getLong("ts"))
        assertEquals(
            t.getString("welcome_bytes"),
            LinkCrypto.welcomeBytes(hs.helloData, b64("sn"), tsS, b64("server_epk")).hex(),
        )

        val session = hs.finish(2, v.getString("sn"), tsS, v.getString("server_epk"), t.getString("welcome_mac"))

        // Klient -> HA: ta sama ramka co z FrameCodec w Pythonie
        val c2s = t.getJSONObject("c2s_frame")
        val (n, p) = session.tx.encrypt(t.getString("c2s_plain"))
        assertEquals(c2s.getLong("n"), n)
        assertEquals(c2s.getString("p"), p)

        // HA -> klient: deszyfrujemy ramke wyprodukowana przez Pythona
        val s2c = t.getJSONObject("s2c_frame")
        assertEquals(t.getString("s2c_plain"), session.rx.decrypt(s2c.getLong("n"), s2c.getString("p")))
        // Replay tej samej ramki musi byc odrzucony
        try {
            session.rx.decrypt(s2c.getLong("n"), s2c.getString("p"))
            fail("replay przeszedl")
        } catch (e: IllegalStateException) {
            assertTrue(e.message!!.contains("replay"))
        }
    }

    @Test
    fun wrongKeyRejectsWelcome() {
        val t = v.getJSONObject("plain")
        val hs = ClientHandshake(
            psk = ByteArray(32) { 7 },
            node = v.getString("node"),
            nowSeconds = { v.getLong("ts_c") },
            cn = b64("cn"),
            ephemeralPrivate = b64("client_priv"),
        )
        try {
            hs.finish(2, v.getString("sn"), v.getLong("ts_s"), v.getString("server_epk"), t.getString("welcome_mac"))
            fail("zly klucz przeszedl")
        } catch (e: ClientHandshake.HandshakeError) {
            assertEquals("auth", e.reason)
        }
    }

    @Test
    fun clockSkewRejected() {
        val t = v.getJSONObject("plain")
        val hs = ClientHandshake(
            psk = b64("psk"),
            node = v.getString("node"),
            nowSeconds = { v.getLong("ts_c") + 1000 },
            cn = b64("cn"),
            ephemeralPrivate = b64("client_priv"),
        )
        try {
            hs.finish(2, v.getString("sn"), v.getLong("ts_s"), v.getString("server_epk"), t.getString("welcome_mac"))
            fail("rozjechany zegar przeszedl")
        } catch (e: ClientHandshake.HandshakeError) {
            assertEquals("clock", e.reason)
        }
    }
}
