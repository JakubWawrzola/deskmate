package com.wawrzola.deskmatetv

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.ConnectivityManager
import android.net.Network
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.util.Log
import com.wawrzola.deskmatetv.link.LinkClient
import com.wawrzola.deskmatetv.service.TvAccessibilityService
import com.wawrzola.deskmatetv.tv.Apps
import com.wawrzola.deskmatetv.tv.Media
import com.wawrzola.deskmatetv.tv.OpError
import com.wawrzola.deskmatetv.tv.Volume
import org.json.JSONArray
import org.json.JSONObject
import java.util.Base64
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.Executors

/**
 * Rdzen aplikacji: jedno polaczenie Link na proces, stan do HA (aplikacja,
 * co gra, glosnosc), obsluga polecen. Startuje z App/uslug/odbiornikow -
 * kto pierwszy.
 */
object Agent : LinkClient.Listener {
    private const val TAG = "DeskmateTV"
    private const val VOLUME_CHANGED = "android.media.VOLUME_CHANGED_ACTION"
    private const val MUTE_CHANGED = "android.media.STREAM_MUTE_CHANGED_ACTION"

    lateinit var app: Context
        private set
    lateinit var cfg: Config
        private set
    lateinit var link: LinkClient
        private set

    private val ops = Executors.newFixedThreadPool(3) { Thread(it, "tv-op") }
    private val state = ConcurrentHashMap<String, Any>()
    private val main = Handler(Looper.getMainLooper())
    private val volumeRunnable = Runnable { publishVolume() }
    @Volatile private var initialized = false

    @Synchronized
    fun init(context: Context) {
        if (initialized) return
        app = context.applicationContext
        cfg = Config(app)
        link = LinkClient(cfg, this)
        registerSystemCallbacks()
        initialized = true
        if (cfg.isConfigured) link.start()
    }

    /** Upewnij sie, ze polaczenie zyje (boot, watchdog, wlaczenie ekranu). */
    fun kick(context: Context) {
        init(context)
        if (!cfg.isConfigured) return
        link.start()
        link.reconnectNow()
    }

    /** Po nowym parowaniu: zerwij sesje i polacz z nowymi danymi. */
    fun reconfigured(context: Context) {
        init(context)
        link.start()
        link.reconnectNow(force = true)
    }

    private fun registerSystemCallbacks() {
        // Wybudzenie TV i powrot sieci = natychmiastowa proba zamiast czekania na backoff
        val filter = IntentFilter().apply {
            addAction(Intent.ACTION_SCREEN_ON)
            addAction(Intent.ACTION_SCREEN_OFF)
            addAction(VOLUME_CHANGED)
            addAction(MUTE_CHANGED)
        }
        val receiver = object : BroadcastReceiver() {
            override fun onReceive(context: Context, intent: Intent) {
                when (intent.action) {
                    Intent.ACTION_SCREEN_ON -> if (cfg.isConfigured) link.reconnectNow()
                    VOLUME_CHANGED, MUTE_CHANGED -> {
                        main.removeCallbacks(volumeRunnable)
                        main.postDelayed(volumeRunnable, 400)
                    }
                }
            }
        }
        if (Build.VERSION.SDK_INT >= 33) {
            app.registerReceiver(receiver, filter, Context.RECEIVER_NOT_EXPORTED)
        } else {
            app.registerReceiver(receiver, filter)
        }
        try {
            app.getSystemService(ConnectivityManager::class.java)
                .registerDefaultNetworkCallback(object : ConnectivityManager.NetworkCallback() {
                    override fun onAvailable(network: Network) {
                        if (cfg.isConfigured) link.reconnectNow()
                    }
                })
        } catch (e: Exception) {
            Log.w(TAG, "network callback", e)
        }
    }

    // ── zdarzenia z uslug ────────────────────────────────────────

    fun onAccessibility(context: Context, connected: Boolean) {
        init(context)
        if (connected) {
            TvAccessibilityService.instance?.foregroundPkg?.let { onForeground(context, it) }
            kick(context)
        }
    }

    fun onForeground(context: Context, pkg: String) {
        pushState(mapOf("app" to Apps.label(context, pkg)))
    }

    fun onMedia(context: Context) {
        val c = Media.current(context)
        val title = c?.let { Media.title(it) }.orEmpty()
        val playback = Media.stateName(c?.playbackState?.state)
        pushState(mapOf("media_title" to title.take(250), "playback" to playback))
    }

    private fun volumeState(): Map<String, Any> = try {
        val v = Volume.get(app)
        mapOf("volume" to v.optInt("level"), "mute" to v.optBoolean("muted"))
    } catch (_: Exception) {
        emptyMap()
    }

    private fun publishVolume() {
        if (initialized) pushState(volumeState())
    }

    private fun pushState(values: Map<String, Any>) {
        val changed = JSONObject()
        for ((k, v) in values) {
            if (state[k] != v) {
                state[k] = v
                changed.put(k, v)
            }
        }
        if (changed.length() > 0 && initialized && link.isOnline()) {
            link.send(JSONObject().put("t", "state").put("s", changed))
        }
    }

    // ── Link ─────────────────────────────────────────────────────

    override fun onConnected() {
        link.send(declare())
        TvAccessibilityService.instance?.foregroundPkg?.let { state["app"] = Apps.label(app, it) }
        val c = Media.current(app)
        state["media_title"] = c?.let { Media.title(it) }.orEmpty().take(250)
        state["playback"] = Media.stateName(c?.playbackState?.state)
        state.putAll(volumeState())
        val s = JSONObject()
        for ((k, v) in state) s.put(k, v)
        link.send(JSONObject().put("t", "state").put("s", s))
    }

    override fun onDisconnected(reason: String) {
        Log.i(TAG, "Link rozlaczony: $reason")
    }

    override fun onMessage(msg: JSONObject) {
        when (msg.optString("t")) {
            "tv" -> ops.execute { handleOp(msg) }
            "notify" -> ops.execute { handleNotify(msg) }
            "cmd" -> ops.execute { handleCommand(msg) }
            // Deskmate Files dotyczy komputerow; szybka odmowa zamiast 15 s czekania w HA
            "fs" -> link.send(
                JSONObject().put("t", "fs_res").put("id", msg.opt("id")).put("ok", false)
                    .put("error", "DeskmateTV nie udostepnia plikow"),
            )
        }
    }

    private fun handleOp(msg: JSONObject) {
        val id = msg.opt("id")
        val reply = try {
            Ops.handle(app, msg).put("ok", true)
        } catch (e: OpError) {
            (e.extra ?: JSONObject()).put("ok", false).put("code", e.code).put("error", e.message ?: e.code)
        } catch (e: Exception) {
            Log.e(TAG, "op ${msg.optString("op")}", e)
            JSONObject().put("ok", false).put("code", "failed").put("error", "${e.javaClass.simpleName}: ${e.message}")
        }
        link.send(reply.put("t", "tv_res").put("id", id))
    }

    /** Encje number/switch z HA: suwak glosnosci i wyciszenie. */
    private fun handleCommand(msg: JSONObject) {
        val ack = JSONObject().put("t", "ack").put("id", msg.opt("id"))
        try {
            when (msg.optString("key")) {
                "volume" -> Volume.apply(app, msg.optDouble("value", -1.0).toInt().coerceIn(0, 100), null, null)
                "mute" -> Volume.apply(app, null, null, msg.optBoolean("value"))
                else -> throw OpError("bad_args", "nieznana encja ${msg.optString("key")}")
            }
            publishVolume()
            link.send(ack.put("ok", true))
        } catch (e: Exception) {
            link.send(ack.put("ok", false).put("error", e.message ?: e.javaClass.simpleName))
        }
    }

    private fun decodeImage(value: String?): Bitmap? {
        if (value.isNullOrBlank() || !value.startsWith("data:")) return null
        return try {
            val raw = Base64.getDecoder().decode(value.substringAfter(","))
            if (raw.size > 4 * 1024 * 1024) return null
            BitmapFactory.decodeByteArray(raw, 0, raw.size)
        } catch (_: Exception) {
            null
        }
    }

    private fun handleNotify(msg: JSONObject) {
        val svc = TvAccessibilityService.instance
        val ack = JSONObject().put("t", "ack").put("id", msg.opt("id"))
        if (svc == null) {
            link.send(ack.put("ok", false).put("error", "usluga dostepnosci nie dziala"))
            return
        }
        val image = decodeImage(msg.optString("image"))
        val seconds = msg.optInt("seconds", if (image != null) 15 else 10)
        svc.overlay.showBubble(msg.optString("title"), msg.optString("message"), seconds, image)
        link.send(ack.put("ok", true))
    }

    private fun declare(): JSONObject {
        val version = try {
            app.packageManager.getPackageInfo(app.packageName, 0).versionName
        } catch (_: Exception) {
            "?"
        }
        val pl = app.resources.configuration.locales[0].language == "pl"
        fun name(en: String, plName: String) = if (pl) plName else en
        val entities = JSONArray()
            .put(
                JSONObject().put("key", "link").put("kind", "binary_sensor")
                    .put("name", name("Connection", "Połączenie"))
                    .put("device_class", "connectivity").put("link", true),
            )
            .put(JSONObject().put("key", "app").put("kind", "sensor").put("name", name("App", "Aplikacja")).put("icon", "mdi:television-play"))
            .put(JSONObject().put("key", "media_title").put("kind", "sensor").put("name", name("Title", "Tytuł")).put("icon", "mdi:movie-open"))
            .put(JSONObject().put("key", "playback").put("kind", "sensor").put("name", name("Playback", "Odtwarzanie")).put("icon", "mdi:play-pause"))
            .put(
                JSONObject().put("key", "volume").put("kind", "number").put("name", name("Volume", "Głośność"))
                    .put("icon", "mdi:volume-high").put("min", 0).put("max", 100).put("step", 1).put("unit", "%"),
            )
            .put(JSONObject().put("key", "mute").put("kind", "switch").put("name", name("Mute", "Wyciszenie")).put("icon", "mdi:volume-off"))
        return JSONObject()
            .put("t", "declare")
            .put(
                "device",
                JSONObject()
                    .put("name", cfg.deviceName)
                    .put("model", "DeskmateTV (${Build.MANUFACTURER} ${Build.MODEL})")
                    .put("sw_version", version)
                    .put("type", "tv"),
            )
            .put("entities", entities)
    }
}
