package com.wawrzola.deskmatetv.service

import android.Manifest
import android.app.job.JobInfo
import android.app.job.JobParameters
import android.app.job.JobScheduler
import android.app.job.JobService
import android.content.BroadcastReceiver
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.provider.Settings
import android.util.Log
import com.wawrzola.deskmatetv.Agent

/**
 * Pilnuje, zeby usluga dostepnosci i dostep do powiadomien byly wlaczone.
 * Uprawnienie WRITE_SECURE_SETTINGS nadaje raz install.ps1 przez ADB; dziala
 * dalej po wylaczeniu debugowania (pendrive TV nie dziala z debugowaniem).
 */
object Watchdog {
    private const val TAG = "DeskmateTV"
    private const val JOB_ID = 4711
    private const val KEY_A11Y = "enabled_accessibility_services"
    private const val KEY_LISTENERS = "enabled_notification_listeners"
    @Volatile private var missedChecks = 0

    fun canWriteSecure(ctx: Context): Boolean =
        ctx.checkSelfPermission(Manifest.permission.WRITE_SECURE_SETTINGS) == PackageManager.PERMISSION_GRANTED

    fun a11yComponent(ctx: Context) = ComponentName(ctx, TvAccessibilityService::class.java)
    fun listenerComponent(ctx: Context) = ComponentName(ctx, MediaListenerService::class.java)

    private fun contains(value: String?, comp: ComponentName): Boolean {
        if (value.isNullOrBlank()) return false
        val full = comp.flattenToString()
        val short = comp.flattenToShortString()
        return value.split(':').any { it.equals(full, true) || it.equals(short, true) }
    }

    fun a11yEnabled(ctx: Context): Boolean =
        contains(Settings.Secure.getString(ctx.contentResolver, KEY_A11Y), a11yComponent(ctx))

    fun listenerEnabled(ctx: Context): Boolean =
        contains(Settings.Secure.getString(ctx.contentResolver, KEY_LISTENERS), listenerComponent(ctx))

    /** Wlacza brakujace uslugi. Zwraca opis zmian (do logu/diagnostyki). */
    fun ensure(ctx: Context): String {
        if (!canWriteSecure(ctx)) return "brak WRITE_SECURE_SETTINGS"
        val cr = ctx.contentResolver
        val changes = ArrayList<String>()
        try {
            val a11y = a11yComponent(ctx).flattenToString()
            val cur = Settings.Secure.getString(cr, KEY_A11Y).orEmpty()
            if (!a11yEnabled(ctx)) {
                Settings.Secure.putString(cr, KEY_A11Y, join(cur, a11y))
                Settings.Secure.putInt(cr, Settings.Secure.ACCESSIBILITY_ENABLED, 1)
                changes += "dostepnosc wlaczona"
                missedChecks = 0
            } else if (TvAccessibilityService.instance == null) {
                // Wpisana, ale nie zwiazana (np. po awarii) - po dwoch kontrolach
                // przelacz wpis, zeby system zwiazal usluge od nowa.
                missedChecks++
                if (missedChecks >= 2) {
                    Settings.Secure.putString(cr, KEY_A11Y, remove(cur, a11yComponent(ctx)))
                    Thread.sleep(600)
                    Settings.Secure.putString(cr, KEY_A11Y, join(remove(cur, a11yComponent(ctx)), a11y))
                    Settings.Secure.putInt(cr, Settings.Secure.ACCESSIBILITY_ENABLED, 1)
                    changes += "dostepnosc przelaczona"
                    missedChecks = 0
                }
            } else {
                missedChecks = 0
            }
            if (!listenerEnabled(ctx)) {
                val curL = Settings.Secure.getString(cr, KEY_LISTENERS).orEmpty()
                Settings.Secure.putString(cr, KEY_LISTENERS, join(curL, listenerComponent(ctx).flattenToString()))
                changes += "dostep do powiadomien wlaczony"
            }
        } catch (e: Exception) {
            Log.w(TAG, "watchdog", e)
            return "blad: ${e.message}"
        }
        if (changes.isNotEmpty()) Log.i(TAG, "watchdog: ${changes.joinToString()}")
        return changes.joinToString().ifEmpty { "ok" }
    }

    private fun join(cur: String, add: String): String =
        if (cur.isBlank()) add else "$cur:$add"

    private fun remove(cur: String, comp: ComponentName): String =
        cur.split(':').filter {
            it.isNotBlank() && !it.equals(comp.flattenToString(), true) && !it.equals(comp.flattenToShortString(), true)
        }.joinToString(":")

    fun schedule(ctx: Context) {
        val js = ctx.getSystemService(JobScheduler::class.java)
        if (js.getPendingJob(JOB_ID) != null) return
        val job = JobInfo.Builder(JOB_ID, ComponentName(ctx, WatchdogJob::class.java))
            .setPeriodic(15 * 60 * 1000L)
            .setPersisted(true)
            .build()
        js.schedule(job)
    }
}

class WatchdogJob : JobService() {
    override fun onStartJob(params: JobParameters?): Boolean {
        Thread {
            Watchdog.ensure(applicationContext)
            Agent.kick(applicationContext)
            jobFinished(params, false)
        }.start()
        return true
    }

    override fun onStopJob(params: JobParameters?): Boolean = false
}

/** Start po uruchomieniu TV i po aktualizacji aplikacji. */
class BootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val app = context.applicationContext
        Watchdog.schedule(app)
        val pending = goAsync()
        Thread {
            try {
                Watchdog.ensure(app)
                Agent.kick(app)
            } finally {
                pending.finish()
            }
        }.start()
    }
}
