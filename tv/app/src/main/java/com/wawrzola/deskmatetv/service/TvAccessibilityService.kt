package com.wawrzola.deskmatetv.service

import android.accessibilityservice.AccessibilityService
import android.view.accessibility.AccessibilityEvent
import com.wawrzola.deskmatetv.Agent
import com.wawrzola.deskmatetv.tv.Actions
import com.wawrzola.deskmatetv.tv.Apps
import com.wawrzola.deskmatetv.tv.Overlay
import com.wawrzola.deskmatetv.tv.ScreenReader
import com.wawrzola.deskmatetv.update.Updater
import java.util.concurrent.atomic.AtomicLong

/**
 * Usluga dostepnosci: oczy i rece Jarvisa na TV. Typ informacji zwrotnej
 * "generic" - zadnej mowy ani dzwiekow (to nie jest TalkBack).
 */
class TvAccessibilityService : AccessibilityService() {
    val events = AtomicLong()
    @Volatile var foregroundPkg: String? = null
        private set

    lateinit var reader: ScreenReader
        private set
    lateinit var actions: Actions
        private set
    lateinit var overlay: Overlay
        private set

    override fun onServiceConnected() {
        super.onServiceConnected()
        reader = ScreenReader(this)
        actions = Actions(this, reader)
        overlay = Overlay(this)
        instance = this
        rootInActiveWindow?.packageName?.toString()?.let { updateForeground(it) }
        Agent.onAccessibility(this, true)
    }

    override fun onAccessibilityEvent(event: AccessibilityEvent?) {
        if (event == null) return
        events.incrementAndGet()
        if (event.eventType == AccessibilityEvent.TYPE_WINDOW_STATE_CHANGED) {
            val pkg = event.packageName?.toString() ?: return
            Updater.onInstallerWindow(this, pkg)
            updateForeground(pkg)
        }
    }

    private fun updateForeground(pkg: String) {
        if (pkg == foregroundPkg || pkg == packageName || pkg in IGNORED) return
        if (!Apps.isApp(this, pkg)) return
        foregroundPkg = pkg
        Agent.onForeground(this, pkg)
    }

    override fun onInterrupt() = Unit

    override fun onDestroy() {
        if (instance === this) instance = null
        if (::overlay.isInitialized) overlay.removeAll()
        Agent.onAccessibility(this, false)
        super.onDestroy()
    }

    companion object {
        @Volatile var instance: TvAccessibilityService? = null
            private set
        private val IGNORED = setOf("android", "com.android.systemui")
    }
}
