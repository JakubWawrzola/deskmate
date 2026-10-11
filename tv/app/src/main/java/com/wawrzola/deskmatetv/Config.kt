package com.wawrzola.deskmatetv

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import org.json.JSONObject
import java.security.KeyStore
import java.util.Base64
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * Konfiguracja parowania. Klucze (PSK, kaskada) leza w SharedPreferences
 * zaszyfrowane kluczem z Android Keystore - zrzut prefs nie zdradza PSK.
 */
class Config(context: Context) {
    private val prefs = context.getSharedPreferences("config", Context.MODE_PRIVATE)

    val node: String get() = prefs.getString(KEY_NODE, null) ?: DEFAULT_NODE
    val deviceName: String get() = prefs.getString(KEY_NAME, null) ?: DEFAULT_NAME
    /** Imie asystenta we wskazniku na ekranie ("Jarvis steruje"). */
    val assistantName: String get() = prefs.getString(KEY_ASSISTANT, null) ?: DEFAULT_ASSISTANT
    val urlLocal: String? get() = prefs.getString(KEY_URL_LOCAL, null)
    val urlRemote: String? get() = prefs.getString(KEY_URL_REMOTE, null)

    val isConfigured: Boolean get() = psk() != null && !urlLocal.isNullOrBlank()

    fun psk(): ByteArray? = prefs.getString(KEY_PSK, null)?.let { KeystoreBox.open(it) }?.takeIf { it.size == 32 }

    fun cascade(): ByteArray? = prefs.getString(KEY_CASCADE, null)?.let { KeystoreBox.open(it) }?.takeIf { it.size == 32 }

    /** Lista adresow WebSocket do proby (lokalny pierwszy). */
    fun endpoints(): List<String> = listOfNotNull(urlLocal, urlRemote)
        .filter { it.isNotBlank() }
        .map { endpoint(it) }
        .distinct()

    /**
     * Zapis parowania. [code] to kod `DMP1.` z HA (klucz + adresy) albo
     * sam klucz base64. Zwraca opis do wyniku rozgloszenia (bez sekretow).
     */
    fun applyPairing(
        code: String?,
        psk: String?,
        node: String?,
        name: String?,
        url: String?,
        cascade: String?,
        assistant: String? = null,
    ): String {
        var keyB64 = psk?.trim()
        var local = url?.trim()
        var remote: String? = null
        if (!code.isNullOrBlank()) {
            val parsed = parsePairingCode(code.trim())
            keyB64 = parsed.optString("k").ifBlank { keyB64 }
            if (local.isNullOrBlank()) local = parsed.optString("u").ifBlank { null }
            remote = parsed.optString("r").ifBlank { null }
        }
        val key = keyB64?.let { decodeKey(it) } ?: throw IllegalArgumentException("brak poprawnego klucza (32 B base64)")
        if (local.isNullOrBlank()) throw IllegalArgumentException("brak adresu HA (url albo kod DMP1 z polem u)")
        val nodeId = (node?.trim()?.lowercase() ?: this.node)
        require(NODE_RE.matches(nodeId)) { "node_id musi pasowac do [a-z0-9_]{1,64}" }
        val editor = prefs.edit()
            .putString(KEY_PSK, KeystoreBox.seal(key))
            .putString(KEY_URL_LOCAL, local)
            .putString(KEY_NODE, nodeId)
        if (remote != null) editor.putString(KEY_URL_REMOTE, remote) else editor.remove(KEY_URL_REMOTE)
        if (!name.isNullOrBlank()) editor.putString(KEY_NAME, name.trim())
        if (!assistant.isNullOrBlank()) editor.putString(KEY_ASSISTANT, assistant.trim().take(40))
        if (!cascade.isNullOrBlank()) {
            val casc = decodeKey(cascade) ?: throw IllegalArgumentException("zly klucz kaskady")
            editor.putString(KEY_CASCADE, KeystoreBox.seal(casc))
        } else {
            editor.remove(KEY_CASCADE)
        }
        editor.apply()
        return "node=$nodeId url=$local" + (remote?.let { " remote=$it" } ?: "")
    }

    companion object {
        const val DEFAULT_NODE = "tv"
        const val DEFAULT_NAME = "DeskmateTV"
        const val DEFAULT_ASSISTANT = "Jarvis"
        const val WS_PATH = "/api/deskmate_link/ws"
        private val NODE_RE = Regex("^[a-z0-9_]{1,64}$")
        private const val KEY_NODE = "node"
        private const val KEY_NAME = "name"
        private const val KEY_ASSISTANT = "assistant"
        private const val KEY_URL_LOCAL = "url_local"
        private const val KEY_URL_REMOTE = "url_remote"
        private const val KEY_PSK = "psk"
        private const val KEY_CASCADE = "cascade"

        fun endpoint(base: String): String {
            var url = base.trim().trimEnd('/')
            if (url.startsWith("http://")) url = "ws://" + url.removePrefix("http://")
            if (url.startsWith("https://")) url = "wss://" + url.removePrefix("https://")
            return if (url.contains("/api/")) url else url + WS_PATH
        }

        fun parsePairingCode(code: String): JSONObject {
            require(code.startsWith("DMP1.")) { "kod parowania musi zaczynac sie od DMP1." }
            val raw = Base64.getUrlDecoder().decode(code.removePrefix("DMP1."))
            return JSONObject(String(raw, Charsets.UTF_8))
        }

        private fun decodeKey(b64: String): ByteArray? = try {
            Base64.getDecoder().decode(b64.trim()).takeIf { it.size == 32 }
        } catch (_: IllegalArgumentException) {
            null
        }
    }
}

/** AES-256-GCM kluczem nieeksportowalnym z Android Keystore. */
object KeystoreBox {
    private const val ALIAS = "deskmatetv_secrets"

    private fun key(): SecretKey {
        val ks = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (ks.getKey(ALIAS, null) as? SecretKey)?.let { return it }
        val gen = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        gen.init(
            KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build(),
        )
        return gen.generateKey()
    }

    fun seal(plain: ByteArray): String {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, key())
        return Base64.getEncoder().encodeToString(cipher.iv + cipher.doFinal(plain))
    }

    fun open(sealed: String): ByteArray? = try {
        val raw = Base64.getDecoder().decode(sealed)
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, raw.copyOfRange(0, 12)))
        cipher.doFinal(raw, 12, raw.size - 12)
    } catch (_: Exception) {
        null
    }
}
