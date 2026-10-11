package com.wawrzola.deskmatetv.link

import java.math.BigInteger
import java.security.SecureRandom

/**
 * X25519 wg RFC 7748 (drabina Montgomery'ego na BigInteger).
 *
 * Wlasna implementacja, bo KeyAgreement "XDH" nie jest pewny na kazdym
 * Androidzie TV, a BouncyCastle w systemie jest okrojony. Jeden handshake na
 * polaczenie, wiec wydajnosc nie ma znaczenia. Arytmetyka BigInteger nie jest
 * stalo-czasowa; klucz jest efemeryczny (jeden na sesje) i zyje sekundy.
 */
object X25519 {
    private val P: BigInteger = BigInteger.ONE.shiftLeft(255).subtract(BigInteger.valueOf(19))
    private val A24: BigInteger = BigInteger.valueOf(121665)
    private val P_MINUS_2: BigInteger = P.subtract(BigInteger.valueOf(2))
    private val BASE = ByteArray(32).also { it[0] = 9 }

    fun generatePrivate(random: SecureRandom = SecureRandom()): ByteArray =
        ByteArray(32).also { random.nextBytes(it) }

    fun publicKey(privateKey: ByteArray): ByteArray = scalarMult(privateKey, BASE)

    /** Wspolny sekret albo null dla punktu malego rzedu (same zera). */
    fun sharedSecret(privateKey: ByteArray, peerPublic: ByteArray): ByteArray? {
        val shared = scalarMult(privateKey, peerPublic)
        return if (shared.all { it == 0.toByte() }) null else shared
    }

    fun scalarMult(scalar: ByteArray, u: ByteArray): ByteArray {
        require(scalar.size == 32 && u.size == 32) { "X25519: 32-bajtowe wejscia" }
        val k = decodeScalar(scalar)
        val x1 = decodeU(u)
        var x2 = BigInteger.ONE
        var z2 = BigInteger.ZERO
        var x3 = x1
        var z3 = BigInteger.ONE
        var swap = 0
        for (t in 254 downTo 0) {
            val kt = if (k.testBit(t)) 1 else 0
            swap = swap xor kt
            if (swap == 1) {
                val tx = x2; x2 = x3; x3 = tx
                val tz = z2; z2 = z3; z3 = tz
            }
            swap = kt
            val a = x2.add(z2).mod(P)
            val aa = a.multiply(a).mod(P)
            val b = x2.subtract(z2).mod(P)
            val bb = b.multiply(b).mod(P)
            val e = aa.subtract(bb).mod(P)
            val c = x3.add(z3).mod(P)
            val d = x3.subtract(z3).mod(P)
            val da = d.multiply(a).mod(P)
            val cb = c.multiply(b).mod(P)
            val sum = da.add(cb).mod(P)
            x3 = sum.multiply(sum).mod(P)
            val diff = da.subtract(cb).mod(P)
            z3 = x1.multiply(diff.multiply(diff).mod(P)).mod(P)
            x2 = aa.multiply(bb).mod(P)
            z2 = e.multiply(aa.add(A24.multiply(e)).mod(P)).mod(P)
        }
        if (swap == 1) {
            x2 = x3
            z2 = z3
        }
        val result = x2.multiply(z2.modPow(P_MINUS_2, P)).mod(P)
        return encodeU(result)
    }

    private fun decodeScalar(scalar: ByteArray): BigInteger {
        val k = scalar.copyOf()
        k[0] = (k[0].toInt() and 248).toByte()
        k[31] = (k[31].toInt() and 127).toByte()
        k[31] = (k[31].toInt() or 64).toByte()
        return fromLittleEndian(k)
    }

    private fun decodeU(u: ByteArray): BigInteger {
        val copy = u.copyOf()
        copy[31] = (copy[31].toInt() and 127).toByte()
        return fromLittleEndian(copy).mod(P)
    }

    private fun fromLittleEndian(bytes: ByteArray): BigInteger =
        BigInteger(1, bytes.reversedArray())

    private fun encodeU(value: BigInteger): ByteArray {
        val be = value.toByteArray()
        val out = ByteArray(32)
        // toByteArray daje big-endian z ewentualnym zerem znaku na poczatku
        for (i in be.indices) {
            val pos = be.size - 1 - i
            if (i < 32) out[i] = be[pos]
        }
        return out
    }
}
