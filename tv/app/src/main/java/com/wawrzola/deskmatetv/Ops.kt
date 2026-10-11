package com.wawrzola.deskmatetv

import android.content.ActivityNotFoundException
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.PowerManager
import com.wawrzola.deskmatetv.service.MediaListenerService
import com.wawrzola.deskmatetv.service.TvAccessibilityService
import com.wawrzola.deskmatetv.service.Watchdog
import com.wawrzola.deskmatetv.tv.Apps
import com.wawrzola.deskmatetv.tv.Guard
import com.wawrzola.deskmatetv.tv.Inputs
import com.wawrzola.deskmatetv.tv.Media
import com.wawrzola.deskmatetv.tv.OpError
import com.wawrzola.deskmatetv.tv.ScreenReader
import com.wawrzola.deskmatetv.tv.Screenshot
import com.wawrzola.deskmatetv.tv.Volume
import com.wawrzola.deskmatetv.update.Updater
import org.json.JSONObject

/** Operacje `{"t":"tv","op":...}` z HA. Kazda zwraca JSON wyniku albo rzuca [OpError]. */
object Ops {
    private val CONTROL_OPS = setOf("click", "key", "text", "scroll", "launch", "intent", "volume", "input")

    fun handle(ctx: Context, a: JSONObject): JSONObject {
        val op = a.optString("op")
        if (op in CONTROL_OPS || (op == "media" && a.optString("action", "state") != "state")) {
            TvAccessibilityService.instance?.overlay?.pulseIndicator()
        }
        return when (op) {
            "info" -> info(ctx)
            "screen" -> svc().reader.read(
                a.optInt("max", 60).coerceIn(5, 300),
                a.optBoolean("geo", false),
                diff = a.optBoolean("diff", false),
                remember = true,
            )
            "inputs" -> JSONObject().put("inputs", Inputs.list(ctx))
            "input" -> input(ctx, a)
            "find" -> svc().reader.find(required(a, "text"))
            "click" -> click(a)
            "key" -> key(a)
            "text" -> text(a)
            "scroll" -> scroll(a)
            "wait" -> waitFor(a)
            "apps" -> JSONObject().put("apps", Apps.list(ctx))
            "launch" -> launch(ctx, a)
            "intent" -> intent(ctx, a)
            "volume" -> Volume.apply(
                ctx,
                if (a.has("set")) a.getInt("set") else null,
                if (a.has("delta")) a.getInt("delta") else null,
                if (a.has("mute")) a.getBoolean("mute") else null,
            )
            "media" -> Media.control(
                ctx,
                a.optString("action", "state"),
                if (a.has("pos")) a.getLong("pos") else null,
                if (a.has("s")) a.getLong("s") else null,
            )
            "shot" -> shot(a)
            "overlay" -> {
                svc().overlay.showBubble(a.optString("title"), a.optString("text"), a.optInt("s", 10))
                JSONObject()
            }
            "update_begin" -> Updater.begin(ctx, a)
            "update_chunk" -> Updater.chunk(a)
            "update_end" -> Updater.end(ctx, a)
            "update_abort" -> Updater.abort()
            else -> throw OpError("bad_op", "nieznana operacja: $op")
        }
    }

    private fun svc(): TvAccessibilityService =
        TvAccessibilityService.instance ?: throw OpError("no_a11y", "usluga dostepnosci DeskmateTV nie dziala")

    private fun required(a: JSONObject, key: String): String =
        a.optString(key).takeIf { it.isNotBlank() } ?: throw OpError("bad_args", "brak pola $key")

    private fun currentPkg(s: TvAccessibilityService): String? = s.foregroundPkg ?: s.reader.lastPkg

    /** Wspolny ogon akcji: czekanie na zmiane i ewentualny nowy odczyt ekranu. */
    private fun after(s: TvAccessibilityService, a: JSONObject, since: Long, out: JSONObject): JSONObject {
        val waitMs = a.optLong("wait_ms", 1500).coerceIn(0, 15000)
        if (waitMs > 0) out.put("changed", s.actions.waitChange(since, waitMs))
        if (a.optBoolean("screen", false)) {
            out.put("screen", s.reader.read(a.optInt("max", 60).coerceIn(5, 300), a.optBoolean("geo", false)).getString("text"))
        }
        return out
    }

    private fun click(a: JSONObject): JSONObject {
        val s = svc()
        val guard = Guard.from(a)
        val node = when {
            a.has("node") -> s.actions.resolve(a.getInt("node"))
            a.has("text") -> s.reader.match(a.getString("text"))?.node
                ?: throw OpError("not_found", "nie ma na ekranie: ${a.getString("text")}")
            else -> throw OpError("bad_args", "podaj node albo text")
        }
        val label = s.actions.labelOf(node)
        guard.check(label, currentPkg(s))?.let {
            throw OpError("confirm", it, JSONObject().put("target", ScreenReader.clean(label)))
        }
        val since = s.actions.eventSeq()
        val method = s.actions.click(node, a.optBoolean("long", false), a.optString("mode", "auto"))
        return after(s, a, since, JSONObject().put("method", method).put("target", ScreenReader.clean(label)))
    }

    private fun key(a: JSONObject): JSONObject {
        val s = svc()
        val name = required(a, "key")
        if (name == "center" || name == "ok") {
            val focused = s.actions.focusedNode()
            if (focused != null) {
                val label = s.actions.labelOf(focused)
                Guard.from(a).check(label, currentPkg(s))?.let {
                    throw OpError("confirm", it, JSONObject().put("target", ScreenReader.clean(label)))
                }
            }
        }
        val since = s.actions.eventSeq()
        if (!s.actions.key(name, a.optInt("n", 1))) {
            throw OpError("unsupported", "klawisz $name niedostepny przez DeskmateTV - uzyj pilota")
        }
        return after(s, a, since, JSONObject().put("key", name))
    }

    private fun text(a: JSONObject): JSONObject {
        val s = svc()
        val value = a.optString("text")
        if (value.isEmpty() || value.length > 500) throw OpError("bad_args", "tekst 1-500 znakow")
        val since = s.actions.eventSeq()
        val method = s.actions.typeText(value, if (a.has("node")) a.getInt("node") else null, a.optBoolean("submit", false))
        return after(s, a, since, JSONObject().put("method", method))
    }

    private fun scroll(a: JSONObject): JSONObject {
        val s = svc()
        val since = s.actions.eventSeq()
        val ok = s.actions.scroll(if (a.has("node")) a.getInt("node") else null, a.optString("dir", "fwd") != "back")
        if (!ok) throw OpError("failed", "lista nie przewinela sie (koniec?)")
        return after(s, a, since, JSONObject())
    }

    private fun waitFor(a: JSONObject): JSONObject {
        val s = svc()
        val ms = a.optLong("ms", 5000).coerceIn(100, 20000)
        val out = JSONObject()
        when (a.optString("for", "change")) {
            "app" -> out.put("ok_app", s.actions.waitApp(required(a, "value"), ms))
            "text" -> {
                val item = s.actions.waitText(required(a, "value"), ms)
                out.put("found", item != null)
                item?.let { out.put("id", it.id).put("label", it.label) }
            }
            else -> out.put("changed", s.actions.waitChange(s.actions.eventSeq(), ms))
        }
        out.put("app", currentPkg(s) ?: "?")
        if (a.optBoolean("screen", false)) {
            out.put("screen", s.reader.read(a.optInt("max", 60).coerceIn(5, 300), a.optBoolean("geo", false)).getString("text"))
        }
        return out
    }

    private fun startActivity(ctx: Context, intent: Intent) {
        intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        // Kontekst uslugi dostepnosci jest zwolniony z blokady startu aktywnosci w tle
        val starter: Context = TvAccessibilityService.instance ?: ctx
        try {
            starter.startActivity(intent)
        } catch (e: ActivityNotFoundException) {
            throw OpError("not_found", "brak aplikacji obslugujacej: ${intent.data ?: intent.action}")
        } catch (e: SecurityException) {
            throw OpError("denied", "system zablokowal start: ${e.message}")
        }
    }

    private fun launch(ctx: Context, a: JSONObject): JSONObject {
        val pkg = required(a, "pkg")
        Guard.from(a).checkPackage(pkg)?.let { throw OpError("confirm", it, JSONObject().put("target", pkg)) }
        val intent = Apps.launchIntent(ctx, pkg) ?: throw OpError("not_found", "aplikacja $pkg nie jest zainstalowana")
        startActivity(ctx, intent)
        return afterLaunch(a, pkg, JSONObject().put("pkg", pkg).put("label", Apps.label(ctx, pkg)))
    }

    private fun afterLaunch(a: JSONObject, pkg: String?, out: JSONObject): JSONObject {
        val s = TvAccessibilityService.instance ?: return out
        val ms = a.optLong("wait_ms", 6000).coerceIn(0, 20000)
        if (pkg != null && ms > 0) out.put("ready", s.actions.waitApp(pkg, ms))
        if (ms > 0) s.actions.waitChange(s.actions.eventSeq(), minOf(ms, 2500))
        out.put("app", currentPkg(s) ?: "?")
        if (a.optBoolean("screen", false)) {
            out.put("screen", s.reader.read(a.optInt("max", 60).coerceIn(5, 300), a.optBoolean("geo", false)).getString("text"))
        }
        return out
    }

    private fun intent(ctx: Context, a: JSONObject): JSONObject {
        val action = a.optString("action").ifBlank { Intent.ACTION_VIEW }
        val pkg = a.optString("pkg").ifBlank { null }
        val guard = Guard.from(a)
        if (!guard.confirmed && (action.contains("DELETE") || action.contains("UNINSTALL") || action.contains("FACTORY"))) {
            throw OpError("confirm", "akcja $action wymaga potwierdzenia", JSONObject().put("target", action))
        }
        guard.checkPackage(pkg)?.let { throw OpError("confirm", it, JSONObject().put("target", pkg)) }
        val intent = Intent(action)
        a.optString("data").takeIf { it.isNotBlank() }?.let { intent.data = Uri.parse(it) }
        if (pkg != null) intent.setPackage(pkg)
        a.optString("cls").takeIf { it.isNotBlank() && pkg != null }?.let { intent.component = ComponentName(pkg!!, it) }
        a.optJSONArray("categories")?.let { arr -> for (i in 0 until arr.length()) intent.addCategory(arr.getString(i)) }
        a.optJSONObject("extras")?.let { extras ->
            for (key in extras.keys()) {
                when (val v = extras.get(key)) {
                    is Boolean -> intent.putExtra(key, v)
                    is Int -> intent.putExtra(key, v)
                    is Long -> intent.putExtra(key, v)
                    is Double -> intent.putExtra(key, v)
                    else -> intent.putExtra(key, v.toString())
                }
            }
        }
        if (a.optBoolean("clear_task", false)) intent.addFlags(Intent.FLAG_ACTIVITY_CLEAR_TASK)
        startActivity(ctx, intent)
        return afterLaunch(a, pkg, JSONObject().put("started", true))
    }

    private fun input(ctx: Context, a: JSONObject): JSONObject {
        val id = required(a, "id")
        val intent = Inputs.switchIntent(ctx, id) ?: throw OpError("not_found", "nie ma wejscia $id")
        startActivity(ctx, intent)
        return afterLaunch(a, null, JSONObject().put("input", id))
    }

    private fun shot(a: JSONObject): JSONObject {
        val (data, size) = Screenshot.take(svc(), a.optInt("w", 1280), a.optInt("q", 70))
        return JSONObject().put("mime", "image/jpeg").put("data", data).put("w", size[0]).put("h", size[1])
    }

    fun info(ctx: Context): JSONObject {
        val pm = ctx.packageManager
        val pi = pm.getPackageInfo(ctx.packageName, 0)
        val s = TvAccessibilityService.instance
        val pkg = s?.foregroundPkg
        val out = JSONObject()
            .put("version", pi.versionName)
            .put("version_code", if (Build.VERSION.SDK_INT >= 28) pi.longVersionCode else 0)
            .put("sdk", Build.VERSION.SDK_INT)
            .put("model", "${Build.MANUFACTURER} ${Build.MODEL}")
            .put("node", Agent.cfg.node)
            .put("a11y", s != null)
            .put("a11y_enabled", Watchdog.a11yEnabled(ctx))
            .put("media", MediaListenerService.connected && Media.available(ctx))
            .put("write_secure", Watchdog.canWriteSecure(ctx))
            .put("install_allowed", pm.canRequestPackageInstalls())
            .put("dpad", Build.VERSION.SDK_INT >= 33)
            .put("screenshot", Build.VERSION.SDK_INT >= 30)
            .put("screen_on", ctx.getSystemService(PowerManager::class.java).isInteractive)
            .put("app", pkg ?: "?")
            .put("app_label", pkg?.let { Apps.label(ctx, it) } ?: "?")
            .put("update", Updater.lastStatus(ctx))
        try {
            out.put("volume", Volume.get(ctx))
        } catch (_: Exception) {
        }
        return out
    }
}
