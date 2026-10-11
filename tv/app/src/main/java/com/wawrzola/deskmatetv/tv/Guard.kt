package com.wawrzola.deskmatetv.tv

import org.json.JSONObject
import java.util.regex.Pattern

/**
 * Polityka potwierdzen. Wzorce przychodza z HA w kazdym poleceniu (polityka
 * zyje w HA i zmienia sie bez aktualizacji aplikacji):
 *  - `deny`: zawsze wymagaja `confirm:true` (kup, wypozycz, odinstaluj...),
 *  - `strict` + `strict_pkgs`: dodatkowe wzorce w aplikacjach ustawien.
 * Bez listy z HA dziala minimalny zestaw wbudowany.
 */
class Guard private constructor(
    private val deny: List<Pattern>,
    private val strict: List<Pattern>,
    private val strictPkgs: Set<String>,
    val confirmed: Boolean,
) {
    /** Powod blokady albo null, gdy wolno. */
    fun check(label: String, pkg: String?): String? {
        if (confirmed) return null
        val text = label.trim()
        if (text.isEmpty()) return null
        deny.firstOrNull { it.matcher(text).find() }?.let { return "\"$text\" wymaga potwierdzenia" }
        if (pkg != null && pkg in strictPkgs) {
            strict.firstOrNull { it.matcher(text).find() }?.let { return "\"$text\" w ustawieniach wymaga potwierdzenia" }
        }
        return null
    }

    fun checkPackage(pkg: String?): String? {
        if (confirmed || pkg == null) return null
        return if (pkg in UNINSTALL_PKGS) "otwarcie instalatora pakietow wymaga potwierdzenia" else null
    }

    companion object {
        private val UNINSTALL_PKGS = setOf("com.android.packageinstaller", "com.google.android.packageinstaller")
        private const val FLAGS = Pattern.CASE_INSENSITIVE or Pattern.UNICODE_CASE or Pattern.UNICODE_CHARACTER_CLASS
        private val DEFAULT_DENY = listOf(
            "\\bkup\\w*", "\\bzakup\\w*", "\\bwypo[zż]ycz\\w*", "\\bsubskryb\\w*", "\\bzap[lł]a[cć]\\w*",
            "\\bbuy\\b", "\\brent\\b", "\\bpurchase\\w*", "\\bsubscribe\\w*",
            "\\bodinstaluj\\w*", "\\buninstall\\w*", "\\bfabryczn\\w*", "\\bfactory reset",
            "\\d+[,.]\\d{2}\\s?(z[lł]|pln|€|eur|\\$)",
        )

        fun from(args: JSONObject): Guard {
            fun list(key: String): List<String>? = args.optJSONArray(key)?.let { arr ->
                (0 until arr.length()).map { arr.optString(it) }.filter { it.isNotBlank() }
            }
            val deny = (list("deny") ?: DEFAULT_DENY).mapNotNull { compile(it) }
            val strict = (list("strict") ?: emptyList()).mapNotNull { compile(it) }
            val pkgs = (list("strict_pkgs") ?: emptyList()).toSet()
            return Guard(deny, strict, pkgs, args.optBoolean("confirm", false))
        }

        private fun compile(p: String): Pattern? = try {
            Pattern.compile(p, FLAGS)
        } catch (_: Exception) {
            null
        }
    }
}
