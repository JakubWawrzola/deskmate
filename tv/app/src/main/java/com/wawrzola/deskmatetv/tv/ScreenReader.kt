package com.wawrzola.deskmatetv.tv

import android.accessibilityservice.AccessibilityService
import android.graphics.Rect
import android.os.Build
import android.view.accessibility.AccessibilityNodeInfo
import android.view.accessibility.AccessibilityWindowInfo
import org.json.JSONArray
import org.json.JSONObject
import java.text.Normalizer

/**
 * Zamienia drzewo dostepnosci na zwiezly tekst dla Jarvisa (jak TalkBack,
 * tylko do odczytu przez model, bez dzwieku).
 *
 * Linia: `#7 item "Sex Education" [focus]`. Kontener klikalny sklada tekst
 * dzieci w jedna linie, siatki pojedynczych znakow (klawiatury) zwijaja sie
 * do `#9..#44 keys "abc..."`. Numery dotycza ostatniego odczytu.
 */
class ScreenReader(private val svc: AccessibilityService) {
    class Item(
        val id: Int,
        val node: AccessibilityNodeInfo,
        val role: String,
        val label: String,
        val flags: String,
        val geo: String,
        val window: Int,
    )

    private class Ctx(val geo: Boolean, val w: Int, val h: Int) {
        val items = ArrayList<Item>()
        val lines = ArrayList<Pair<Int, String>>() // (id pierwszego elementu albo 0, linia)
        var focusId = 0
        var window = 0
        var lastLabel = ""
    }

    @Volatile private var cache: List<Item> = emptyList()
    /** Linie ostatniego jawnego odczytu (bez numerow) - do trybu zmian. */
    @Volatile private var previous: Set<String> = emptySet()
    @Volatile var lastPkg: String? = null
        private set

    fun item(id: Int): Item? = cache.getOrNull(id - 1)?.takeIf { it.id == id }

    fun items(): List<Item> = cache

    /** Pelny odczyt ekranu. [max] - ile linii oddac modelowi (reszta tylko w cache). */
    fun read(max: Int = 60, geo: Boolean = false, diff: Boolean = false, remember: Boolean = false): JSONObject {
        val dm = svc.resources.displayMetrics
        val c = Ctx(geo, dm.widthPixels.coerceAtLeast(1), dm.heightPixels.coerceAtLeast(1))
        val roots = roots()
        var appPkg: String? = null
        for ((index, r) in roots.withIndex()) {
            c.window = index
            if (roots.size > 1 && r.title != null) c.lines += 0 to "-- ${r.title} --"
            walk(r.node, c, 0, inside = false)
            if (c.items.size >= HARD_CAP) break
        }
        appPkg = (roots.firstOrNull { it.type == AccessibilityWindowInfo.TYPE_APPLICATION }
            ?: roots.firstOrNull { it.type != AccessibilityWindowInfo.TYPE_INPUT_METHOD })
            ?.node?.packageName?.toString()
        cache = c.items
        lastPkg = appPkg
        val out = StringBuilder()
        val pkg = appPkg ?: "?"
        out.append("app=").append(Apps.label(svc, pkg)).append(" (").append(pkg).append(")")
            .append(" n=").append(c.items.size)
        if (c.focusId > 0) out.append(" focus=#").append(c.focusId)
        var shown = 0
        var cut = false
        val keys = c.lines.map { stripId(it.second) }
        val before = previous
        if (remember) previous = keys.toSet()
        var lines: List<Pair<Int, String>> = c.lines
        if (diff && before.isNotEmpty()) {
            // Tylko linie, ktorych nie bylo w poprzednim odczycie (numery sa nowe)
            val removed = before.count { it !in keys }
            lines = c.lines.filterIndexed { i, _ -> keys[i] !in before }
            out.append(" zmiany: +").append(lines.size).append(" -").append(removed)
        }
        for ((_, line) in lines) {
            if (shown >= max) {
                cut = true
                break
            }
            out.append('\n').append(line)
            shown++
        }
        if (cut && c.focusId > 0 && c.lines.take(max).none { it.first == c.focusId }) {
            c.lines.firstOrNull { it.first == c.focusId }?.let { out.append('\n').append(it.second) }
        }
        if (c.items.isEmpty()) {
            out.append("\n(brak elementow dostepnosci - aplikacja rysuje wlasny obraz; uzyj zrzutu albo pilota)")
        }
        return JSONObject()
            .put("app", pkg)
            .put("text", out.toString())
            .put("n", c.items.size)
            .put("cut", cut)
            .put("focus", c.focusId)
    }

    private fun stripId(line: String): String = ID_PREFIX.replace(line, "")

    /** Wyszukanie po tekscie (bez diakrytykow, bez wielkosci liter). */
    fun find(query: String, limit: Int = 15): JSONObject {
        read(max = 0)
        val q = norm(query)
        val hits = JSONArray()
        for (item in cache) {
            if (norm(item.label).contains(q)) {
                hits.put(JSONObject().put("id", item.id).put("role", item.role).put("text", item.label).put("flags", item.flags))
                if (hits.length() >= limit) break
            }
        }
        return JSONObject().put("app", lastPkg ?: "?").put("hits", hits)
    }

    /** Najlepsze dopasowanie tekstu: dokladne, potem poczatek, potem fragment. */
    fun match(query: String, roles: Set<String>? = null): Item? {
        read(max = 0)
        val q = norm(query)
        val pool = cache.filter { roles == null || it.role in roles }
        return pool.firstOrNull { norm(it.label) == q }
            ?: pool.firstOrNull { norm(it.label).startsWith(q) }
            ?: pool.firstOrNull { norm(it.label).contains(q) }
    }

    // ── okna ─────────────────────────────────────────────────────

    private class Root(val node: AccessibilityNodeInfo, val title: String?, val type: Int)

    private fun roots(): List<Root> {
        val out = ArrayList<Root>()
        val windows = try {
            svc.windows
        } catch (_: Exception) {
            emptyList<AccessibilityWindowInfo>()
        }
        for (w in windows.sortedByDescending { it.layer }) {
            val type = w.type
            if (type == AccessibilityWindowInfo.TYPE_ACCESSIBILITY_OVERLAY) continue
            if (type != AccessibilityWindowInfo.TYPE_APPLICATION &&
                type != AccessibilityWindowInfo.TYPE_SYSTEM &&
                type != AccessibilityWindowInfo.TYPE_INPUT_METHOD
            ) continue
            val root = w.root ?: continue
            if (root.packageName?.toString() == svc.packageName) continue
            val title = when (type) {
                AccessibilityWindowInfo.TYPE_INPUT_METHOD -> "klawiatura ekranowa"
                else -> w.title?.toString()?.takeIf { it.isNotBlank() }
            }
            out += Root(root, title, type)
        }
        if (out.isEmpty()) {
            svc.rootInActiveWindow?.let { out += Root(it, null, AccessibilityWindowInfo.TYPE_APPLICATION) }
        }
        return out
    }

    // ── przejscie drzewa ────────────────────────────────────────

    private fun walk(n: AccessibilityNodeInfo, c: Ctx, depth: Int, inside: Boolean) {
        if (depth > MAX_DEPTH || c.items.size >= HARD_CAP) return
        if (!n.isVisibleToUser) return
        val kids = children(n)
        val actionable = isActionable(n, kids)

        if (!actionable) {
            val keys = kids.filter { it.isVisibleToUser && isKey(it) }
            if (keys.size >= 8 && keys.size * 2 >= kids.size) {
                emitKeys(keys, c)
                for (k in kids) if (k !in keys) walk(k, c, depth + 1, inside)
                return
            }
        }

        if (actionable) {
            val own = ownText(n)
            val label = if (own.isNotBlank()) own else mergedText(kids)
            emit(n, c, role(n, true), label)
            for (k in kids) walk(k, c, depth + 1, inside = true)
            return
        }

        if (n.isScrollable) emit(n, c, "list", ownText(n))
        if (!inside) {
            val own = ownText(n)
            if (own.isNotBlank()) emit(n, c, role(n, false), own)
        }
        for (k in kids) walk(k, c, depth + 1, inside)
    }

    private fun emit(n: AccessibilityNodeInfo, c: Ctx, role: String, rawLabel: String) {
        val label = clean(rawLabel)
        if (role == "txt" && (label.isBlank() || label == c.lastLabel)) return
        val flags = flags(n)
        val geo = if (c.geo) geo(n, c) else ""
        val id = c.items.size + 1
        c.items += Item(id, n, role, label, flags, geo, c.window)
        if (c.focusId == 0 && (n.isFocused || n.isAccessibilityFocused)) c.focusId = id
        val line = StringBuilder("#").append(id).append(' ').append(role)
        if (label.isNotBlank()) line.append(" \"").append(label).append('"')
        if (role == "edit") hint(n)?.let { line.append(" hint=\"").append(clean(it)).append('"') }
        if (flags.isNotEmpty()) line.append(' ').append(flags)
        if (geo.isNotEmpty()) line.append(' ').append(geo)
        c.lines += id to line.toString()
        c.lastLabel = label
    }

    private fun emitKeys(keys: List<AccessibilityNodeInfo>, c: Ctx) {
        val first = c.items.size + 1
        val labels = ArrayList<String>()
        for (k in keys) {
            val label = keyLabel(k) ?: continue
            c.items += Item(c.items.size + 1, k, "key", label, flags(k), "", c.window)
            if (c.focusId == 0 && (k.isFocused || k.isAccessibilityFocused)) c.focusId = c.items.size
            labels += label
        }
        if (labels.isEmpty()) return
        val last = c.items.size
        val joined = if (labels.all { it.length == 1 }) labels.joinToString("") else labels.joinToString(" ")
        c.lines += first to "#$first..#$last keys \"${joined.replace('"', '\'')}\""
    }

    private fun children(n: AccessibilityNodeInfo): List<AccessibilityNodeInfo> {
        val out = ArrayList<AccessibilityNodeInfo>(n.childCount)
        for (i in 0 until n.childCount) n.getChild(i)?.let { out += it }
        return out
    }

    private fun isActionable(n: AccessibilityNodeInfo, kids: List<AccessibilityNodeInfo>): Boolean {
        if (n.isEditable || n.isCheckable) return true
        if (n.isClickable || n.isLongClickable) return !looksLikeContainer(n, kids)
        if (n.isFocusable && !n.isScrollable) return !looksLikeContainer(n, kids)
        return false
    }

    /**
     * Fokusowalny/klikalny rzad z wieloma kartami albo wezel na pol ekranu to
     * kontener, nie element - inaczej "polknalby" caly tekst pod soba.
     */
    private fun looksLikeContainer(n: AccessibilityNodeInfo, kids: List<AccessibilityNodeInfo>): Boolean {
        val cls = n.className?.toString() ?: ""
        if (CONTAINER_CLASSES.any { cls.contains(it) }) return true
        if (kids.isNotEmpty()) {
            val r = Rect()
            n.getBoundsInScreen(r)
            val dm = svc.resources.displayMetrics
            val screen = dm.widthPixels.toLong() * dm.heightPixels.toLong()
            if (screen > 0 && r.width().toLong() * r.height().toLong() * 2 > screen) return true
        }
        var actionableKids = 0
        for (k in kids) {
            if (k.isClickable || k.isFocusable) actionableKids++
            if (actionableKids >= 2) return true
        }
        return false
    }

    private fun isKey(n: AccessibilityNodeInfo): Boolean =
        (n.isClickable || n.isFocusable) && keyLabel(n) != null

    private fun keyLabel(n: AccessibilityNodeInfo): String? {
        val t = (n.text ?: n.contentDescription)?.toString()?.trim() ?: return null
        return t.takeIf { it.length in 1..2 }
    }

    private fun ownText(n: AccessibilityNodeInfo): String {
        val text = n.text?.toString()?.trim().orEmpty()
        val desc = n.contentDescription?.toString()?.trim().orEmpty()
        val state = if (Build.VERSION.SDK_INT >= 30) n.stateDescription?.toString()?.trim().orEmpty() else ""
        val base = when {
            text.isNotEmpty() && desc.isNotEmpty() && !desc.equals(text, true) && desc.length <= 40 -> "$text ($desc)"
            text.isNotEmpty() -> text
            else -> desc
        }
        return if (state.isNotEmpty() && !base.contains(state)) "$base, $state".trim(',', ' ') else base
    }

    private fun mergedText(kids: List<AccessibilityNodeInfo>): String {
        val parts = ArrayList<String>()
        fun collect(n: AccessibilityNodeInfo, depth: Int) {
            if (parts.size >= 3 || depth > 6 || !n.isVisibleToUser) return
            val t = ownText(n)
            if (t.isNotBlank() && parts.none { it == t }) parts += t
            for (i in 0 until n.childCount) {
                val k = n.getChild(i) ?: continue
                if (k.isClickable && k.childCount > 0) continue // zagniezdzony przycisk ma wlasna linie
                collect(k, depth + 1)
            }
        }
        for (k in kids) collect(k, 0)
        return parts.joinToString(" / ")
    }

    private fun role(n: AccessibilityNodeInfo, actionable: Boolean): String {
        val cls = n.className?.toString() ?: ""
        return when {
            n.isEditable || cls.endsWith("EditText") -> "edit"
            n.isCheckable || cls.contains("Switch") || cls.contains("CheckBox") -> "chk"
            cls.contains("Tab") -> "tab"
            cls.contains("SeekBar") -> "bar"
            cls.contains("Button") -> "btn"
            cls.endsWith("ImageView") || cls.endsWith("Image") -> if (actionable) "btn" else "img"
            actionable -> "item"
            else -> "txt"
        }
    }

    private fun flags(n: AccessibilityNodeInfo): String {
        val f = ArrayList<String>(3)
        if (n.isFocused || n.isAccessibilityFocused) f += "focus"
        if (n.isSelected) f += "sel"
        if (n.isCheckable) f += if (n.isChecked) "x" else " "
        if (!n.isEnabled) f += "off"
        if (n.isScrollable) f += "scroll"
        return if (f.isEmpty()) "" else f.joinToString("][", "[", "]")
    }

    private fun hint(n: AccessibilityNodeInfo): String? =
        n.hintText?.toString()?.takeIf { it.isNotBlank() && it != n.text?.toString() }

    private fun geo(n: AccessibilityNodeInfo, c: Ctx): String {
        val r = Rect()
        n.getBoundsInScreen(r)
        val x = (r.centerX() * 100 / c.w).coerceIn(0, 99)
        val y = (r.centerY() * 100 / c.h).coerceIn(0, 99)
        return "@$x,$y"
    }

    companion object {
        private const val HARD_CAP = 400
        private const val MAX_DEPTH = 45
        private const val MAX_LABEL = 80
        private val ID_PREFIX = Regex("""^#\d+(\.\.#\d+)? """)
        private val CONTAINER_CLASSES = listOf("RecyclerView", "ListView", "GridView", "ScrollView", "ViewPager", "HorizontalGridView", "VerticalGridView")

        fun clean(raw: String): String {
            val s = raw.replace('\n', '/').replace('\r', ' ').replace('"', '\'')
                .replace(Regex("\\s+"), " ").trim()
            return if (s.length > MAX_LABEL) s.substring(0, MAX_LABEL - 1) + "~" else s
        }

        fun norm(s: String): String {
            val lower = s.lowercase().replace('ł', 'l')
            return Normalizer.normalize(lower, Normalizer.Form.NFD).replace(Regex("\\p{Mn}+"), "").trim()
        }
    }
}
