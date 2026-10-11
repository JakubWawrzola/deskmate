package com.wawrzola.deskmatetv.tv

import android.accessibilityservice.AccessibilityService
import android.os.Build
import android.os.Bundle
import android.view.accessibility.AccessibilityNodeInfo
import com.wawrzola.deskmatetv.service.TvAccessibilityService

/** Akcje na ekranie: klik, D-pad, tekst, przewijanie, czekanie na zmiane. */
class Actions(private val svc: TvAccessibilityService, private val reader: ScreenReader) {

    val dpadSupported: Boolean get() = Build.VERSION.SDK_INT >= 33

    fun eventSeq(): Long = svc.events.get()

    /** Czeka na zmiane ekranu po [since], potem az ucichnie na [settleMs]. */
    fun waitChange(since: Long, timeoutMs: Long, settleMs: Long = 350): Boolean {
        val deadline = System.currentTimeMillis() + timeoutMs
        while (System.currentTimeMillis() < deadline) {
            val seq = svc.events.get()
            if (seq != since) {
                var last = seq
                var quietFrom = System.currentTimeMillis()
                while (System.currentTimeMillis() < deadline && System.currentTimeMillis() - quietFrom < settleMs) {
                    Thread.sleep(50)
                    val now = svc.events.get()
                    if (now != last) {
                        last = now
                        quietFrom = System.currentTimeMillis()
                    }
                }
                return true
            }
            Thread.sleep(50)
        }
        return false
    }

    /** Czeka, az aplikacja [pkg] bedzie na pierwszym planie. */
    fun waitApp(pkg: String, timeoutMs: Long): Boolean {
        val deadline = System.currentTimeMillis() + timeoutMs
        while (System.currentTimeMillis() < deadline) {
            if (svc.foregroundPkg == pkg) return true
            Thread.sleep(100)
        }
        return svc.foregroundPkg == pkg
    }

    /** Czeka, az na ekranie pojawi sie tekst. */
    fun waitText(text: String, timeoutMs: Long): ScreenReader.Item? {
        val deadline = System.currentTimeMillis() + timeoutMs
        while (true) {
            reader.match(text)?.let { return it }
            if (System.currentTimeMillis() >= deadline) return null
            val seq = svc.events.get()
            waitChange(seq, minOf(800L, deadline - System.currentTimeMillis()).coerceAtLeast(50), 150)
        }
    }

    fun resolve(id: Int): AccessibilityNodeInfo {
        val item = reader.item(id) ?: throw OpError("stale", "nie ma #$id w ostatnim odczycie - odczytaj ekran ponownie")
        if (!item.node.refresh() || !item.node.isVisibleToUser) {
            throw OpError("stale", "#$id zniknal z ekranu - odczytaj ekran ponownie")
        }
        return item.node
    }

    /** Etykieta do sprawdzenia przez Guard: tekst wezla albo jego dzieci. */
    fun labelOf(node: AccessibilityNodeInfo): String {
        val own = listOfNotNull(node.text, node.contentDescription).joinToString(" ").trim()
        if (own.isNotEmpty()) return own
        val parts = ArrayList<String>()
        fun collect(n: AccessibilityNodeInfo, depth: Int) {
            if (parts.size >= 4 || depth > 4) return
            listOfNotNull(n.text, n.contentDescription).map { it.toString().trim() }.filter { it.isNotEmpty() }.forEach { parts += it }
            for (i in 0 until n.childCount) n.getChild(i)?.let { collect(it, depth + 1) }
        }
        collect(node, 0)
        return parts.joinToString(" ")
    }

    /**
     * Klik. "auto": ACTION_CLICK, a gdy ekran nie drgnie - fokus + srodek D-pada
     * (czesc aplikacji TV ignoruje klik z dostepnosci). "center": od razu fokus + srodek.
     */
    fun click(node: AccessibilityNodeInfo, long: Boolean, mode: String): String {
        var target: AccessibilityNodeInfo? = node
        while (target != null && !(target.isClickable || target.isFocusable || target.isLongClickable)) target = target.parent
        val t = target ?: node
        if (mode != "center") {
            val seq = eventSeq()
            val action = if (long) AccessibilityNodeInfo.ACTION_LONG_CLICK else AccessibilityNodeInfo.ACTION_CLICK
            if (t.performAction(action)) {
                if (mode == "click" || long || waitChange(seq, 700, 150)) return "click"
            }
        }
        if (!dpadSupported) {
            throw OpError("unsupported", "klik nie zadzialal, a srodek D-pada wymaga Androida 13 - uzyj pilota (ok)")
        }
        t.performAction(AccessibilityNodeInfo.ACTION_FOCUS)
        Thread.sleep(150)
        svc.performGlobalAction(AccessibilityService.GLOBAL_ACTION_DPAD_CENTER)
        return if (mode == "center") "center" else "click+center"
    }

    fun focusedNode(): AccessibilityNodeInfo? =
        svc.rootInActiveWindow?.findFocus(AccessibilityNodeInfo.FOCUS_INPUT)
            ?: svc.findFocus(AccessibilityNodeInfo.FOCUS_INPUT)

    /** Klawisz. Zwraca false dla nieobslugiwanego (HA uzyje wtedy pilota). */
    fun key(name: String, times: Int): Boolean {
        val action = when (name) {
            "back" -> AccessibilityService.GLOBAL_ACTION_BACK
            "home" -> AccessibilityService.GLOBAL_ACTION_HOME
            "recents" -> AccessibilityService.GLOBAL_ACTION_RECENTS
            "notifications" -> AccessibilityService.GLOBAL_ACTION_NOTIFICATIONS
            "up" -> if (dpadSupported) AccessibilityService.GLOBAL_ACTION_DPAD_UP else return false
            "down" -> if (dpadSupported) AccessibilityService.GLOBAL_ACTION_DPAD_DOWN else return false
            "left" -> if (dpadSupported) AccessibilityService.GLOBAL_ACTION_DPAD_LEFT else return false
            "right" -> if (dpadSupported) AccessibilityService.GLOBAL_ACTION_DPAD_RIGHT else return false
            "center", "ok" -> if (dpadSupported) AccessibilityService.GLOBAL_ACTION_DPAD_CENTER else return false
            else -> return false
        }
        repeat(times.coerceIn(1, 20)) { i ->
            if (i > 0) Thread.sleep(180)
            if (!svc.performGlobalAction(action)) return false
        }
        return true
    }

    /**
     * Wpisanie tekstu. Najpierw ACTION_SET_TEXT na polu edycji; gdy pola nie
     * ma, klikanie liter klawiatury ekranowej aplikacji (np. Netflix).
     */
    fun typeText(text: String, nodeId: Int?, submit: Boolean): String {
        val target = nodeId?.let { resolve(it) }
            ?: focusedNode()?.takeIf { it.isEditable }
            ?: findEditable()
        if (target != null && target.isEditable) {
            target.performAction(AccessibilityNodeInfo.ACTION_FOCUS)
            val args = Bundle().apply {
                putCharSequence(AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE, text)
            }
            if (!target.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, args)) {
                throw OpError("failed", "pole odrzucilo tekst")
            }
            if (submit) {
                val ok = Build.VERSION.SDK_INT >= 30 &&
                    target.performAction(AccessibilityNodeInfo.AccessibilityAction.ACTION_IME_ENTER.id)
                if (!ok) return "set_text (bez Enter - uzyj klawisza ok)"
            }
            return "set_text"
        }
        return typeOnKeyboard(text)
    }

    private fun findEditable(): AccessibilityNodeInfo? {
        reader.read(max = 0)
        return reader.items().firstOrNull { it.node.isEditable }?.node
    }

    private fun typeOnKeyboard(text: String): String {
        reader.read(max = 0)
        val keys = HashMap<String, AccessibilityNodeInfo>()
        for (item in reader.items()) {
            val label = ScreenReader.norm(item.label)
            if (item.role == "key" || label.length <= 2) keys.putIfAbsent(label, item.node)
            if (label in SPACE_LABELS) keys.putIfAbsent(" ", item.node)
        }
        if (keys.size < 10) throw OpError("no_input", "brak pola tekstowego i klawiatury ekranowej - otworz wyszukiwanie")
        val wanted = ScreenReader.norm(text)
        val missing = wanted.toSet().filter { it.toString() !in keys }
        if (missing.isNotEmpty()) {
            throw OpError("no_input", "klawiatura nie ma znakow: ${missing.joinToString("")}")
        }
        for (ch in wanted) {
            val node = keys.getValue(ch.toString())
            if (!node.performAction(AccessibilityNodeInfo.ACTION_CLICK)) {
                node.performAction(AccessibilityNodeInfo.ACTION_FOCUS)
                if (dpadSupported) svc.performGlobalAction(AccessibilityService.GLOBAL_ACTION_DPAD_CENTER)
            }
            Thread.sleep(120)
        }
        return "keyboard"
    }

    fun scroll(nodeId: Int?, forward: Boolean): Boolean {
        val node = nodeId?.let { resolve(it) } ?: run {
            reader.read(max = 0)
            reader.items().filter { it.node.isScrollable }.maxByOrNull {
                val r = android.graphics.Rect()
                it.node.getBoundsInScreen(r)
                r.width() * r.height()
            }?.node
        } ?: throw OpError("not_found", "brak przewijalnej listy")
        val action = if (forward) AccessibilityNodeInfo.ACTION_SCROLL_FORWARD else AccessibilityNodeInfo.ACTION_SCROLL_BACKWARD
        return node.performAction(action)
    }

    companion object {
        private val SPACE_LABELS = setOf("spacja", "space", "odstep", "␣", "spacebar")
    }
}
