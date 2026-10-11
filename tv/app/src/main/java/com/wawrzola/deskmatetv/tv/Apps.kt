package com.wawrzola.deskmatetv.tv

import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import org.json.JSONArray
import org.json.JSONObject
import java.util.concurrent.ConcurrentHashMap

/** Lista zainstalowanych aplikacji, etykiety, uruchamianie. */
object Apps {
    private val labels = ConcurrentHashMap<String, String>()
    private val appPkgs = ConcurrentHashMap<String, Boolean>()

    fun list(ctx: Context): JSONArray {
        val pm = ctx.packageManager
        val seen = LinkedHashMap<String, String>()
        for (category in listOf(Intent.CATEGORY_LEANBACK_LAUNCHER, Intent.CATEGORY_LAUNCHER)) {
            val intent = Intent(Intent.ACTION_MAIN).addCategory(category)
            for (ri in pm.queryIntentActivities(intent, 0)) {
                val pkg = ri.activityInfo?.packageName ?: continue
                if (pkg == ctx.packageName || pkg in seen) continue
                seen[pkg] = ri.loadLabel(pm)?.toString() ?: pkg
            }
        }
        val out = JSONArray()
        seen.entries.sortedBy { it.value.lowercase() }.forEach {
            labels[it.key] = it.value
            out.put(JSONObject().put("pkg", it.key).put("label", it.value))
        }
        return out
    }

    fun launchIntent(ctx: Context, pkg: String): Intent? {
        val pm = ctx.packageManager
        return (pm.getLeanbackLaunchIntentForPackage(pkg) ?: pm.getLaunchIntentForPackage(pkg))
            ?.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_RESET_TASK_IF_NEEDED)
    }

    fun label(ctx: Context, pkg: String): String {
        if (pkg == "?") return "?"
        labels[pkg]?.let { return it }
        val label = try {
            val pm = ctx.packageManager
            pm.getApplicationLabel(pm.getApplicationInfo(pkg, 0)).toString()
        } catch (_: PackageManager.NameNotFoundException) {
            pkg
        }
        labels[pkg] = label
        return label
    }

    /** Czy pakiet to aplikacja pierwszego planu (ma launcher albo jest ekranem glownym). */
    fun isApp(ctx: Context, pkg: String): Boolean = appPkgs.getOrPut(pkg) {
        if (pkg == ctx.packageName) return@getOrPut false
        if (launchIntent(ctx, pkg) != null) return@getOrPut true
        val home = Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_HOME)
        ctx.packageManager.queryIntentActivities(home, 0).any { it.activityInfo?.packageName == pkg }
    }
}
