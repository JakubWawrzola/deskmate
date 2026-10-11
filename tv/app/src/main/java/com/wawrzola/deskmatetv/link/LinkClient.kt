package com.wawrzola.deskmatetv.link

import android.util.Log
import com.wawrzola.deskmatetv.Config
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import org.json.JSONObject
import java.util.concurrent.Executors
import java.util.concurrent.ScheduledFuture
import java.util.concurrent.TimeUnit

/**
 * Klient Deskmate Link v2: WebSocket do HA, handshake, szyfrowane ramki,
 * ping, automatyczne ponowne laczenie z backoffem 2-60 s.
 *
 * Wszystkie zmiany stanu ida przez jeden watek [loop]; callbacki OkHttp
 * z nieaktualnych gniazd sa odrzucane po numerze generacji.
 */
class LinkClient(private val cfg: Config, private val listener: Listener) {
    interface Listener {
        fun onConnected()
        fun onDisconnected(reason: String)
        fun onMessage(msg: JSONObject)
    }

    enum class Phase { IDLE, CONNECTING, HANDSHAKE, ONLINE }

    @Volatile var phase: Phase = Phase.IDLE
        private set
    @Volatile var lastError: String = ""
        private set
    @Volatile var connectedSince: Long = 0
        private set
    @Volatile var endpoint: String = ""
        private set

    private val http = OkHttpClient.Builder()
        .connectTimeout(10, TimeUnit.SECONDS)
        .readTimeout(0, TimeUnit.SECONDS)
        .pingInterval(25, TimeUnit.SECONDS)
        .build()
    private val loop = Executors.newSingleThreadScheduledExecutor { Thread(it, "link") }
    private val sendLock = Any()

    private var running = false
    private var gen = 0
    private var ws: WebSocket? = null
    private var handshake: ClientHandshake? = null
    private var session: LinkSession? = null
    private var attempt = 0
    private var urlIndex = 0
    private var reconnect: ScheduledFuture<*>? = null
    private var welcomeTimeout: ScheduledFuture<*>? = null
    private var pinger: ScheduledFuture<*>? = null
    @Volatile private var lastRx: Long = 0

    fun start() = loop.execute {
        running = true
        if (phase == Phase.IDLE && reconnect == null) connect()
    }

    fun stop() = loop.execute {
        running = false
        reconnect?.cancel(false)
        reconnect = null
        drop("zatrzymano")
    }

    /** Natychmiastowa proba (wlaczenie ekranu, nowa siec, nowe parowanie). */
    fun reconnectNow(force: Boolean = false) = loop.execute {
        if (!running) return@execute
        // Bez force nie przerywamy trwajacej proby ani zywej sesji - kilka
        // zrodel (boot, usluga, siec) potrafi "kopnac" naraz.
        if (!force && phase != Phase.IDLE) return@execute
        reconnect?.cancel(false)
        reconnect = null
        attempt = 0
        if (phase != Phase.IDLE) drop("ponowne laczenie")
        connect()
    }

    fun isOnline(): Boolean = phase == Phase.ONLINE

    /** Szyfruje i wysyla wiadomosc. false = brak sesji. */
    fun send(msg: JSONObject): Boolean {
        synchronized(sendLock) {
            val s = session ?: return false
            val socket = ws ?: return false
            val (n, p) = s.tx.encrypt(msg.toString())
            return socket.send("{\"t\":\"e\",\"n\":$n,\"p\":\"$p\"}")
        }
    }

    // ── wewnetrzne (watek loop) ──────────────────────────────────

    private fun connect() {
        if (!running) return
        val psk = cfg.psk()
        val urls = cfg.endpoints()
        if (psk == null || urls.isEmpty()) {
            lastError = "brak parowania (uruchom install.ps1)"
            phase = Phase.IDLE
            return
        }
        val url = urls[urlIndex % urls.size]
        endpoint = url
        val myGen = ++gen
        phase = Phase.CONNECTING
        handshake = ClientHandshake(psk, cfg.node, cfg.cascade())
        Log.i(TAG, "laczenie z $url")
        ws = http.newWebSocket(Request.Builder().url(url).build(), Callbacks(myGen))
    }

    private fun scheduleReconnect(delaySeconds: Long? = null) {
        if (!running) return
        reconnect?.cancel(false)
        val delay = delaySeconds ?: BACKOFF[minOf(attempt, BACKOFF.size - 1)]
        attempt++
        reconnect = loop.schedule({
            reconnect = null
            connect()
        }, delay, TimeUnit.SECONDS)
    }

    private fun drop(reason: String) {
        welcomeTimeout?.cancel(false)
        pinger?.cancel(false)
        val wasOnline = phase == Phase.ONLINE
        synchronized(sendLock) {
            session = null
            ws?.cancel()
            ws = null
        }
        handshake = null
        gen++
        phase = Phase.IDLE
        connectedSince = 0
        if (wasOnline) listener.onDisconnected(reason)
    }

    private fun fail(reason: String, delaySeconds: Long? = null, rotate: Boolean = false) {
        lastError = reason
        Log.w(TAG, "Link: $reason")
        drop(reason)
        if (rotate) urlIndex++
        scheduleReconnect(delaySeconds)
    }

    private fun onText(text: String) {
        lastRx = System.currentTimeMillis()
        when (phase) {
            Phase.HANDSHAKE -> onWelcome(text)
            Phase.ONLINE -> onFrame(text)
            else -> Unit
        }
    }

    private fun onWelcome(text: String) {
        welcomeTimeout?.cancel(false)
        val msg = try {
            JSONObject(text)
        } catch (_: Exception) {
            fail("nieczytelna odpowiedz HA")
            return
        }
        when (msg.optString("t")) {
            "reject" -> {
                val reason = msg.optString("reason", "auth")
                val delay = when (reason) {
                    "locked" -> 300L
                    "auth", "version", "cascade", "clock" -> 60L
                    else -> null
                }
                fail("HA odrzucil polaczenie: $reason", delay)
            }
            "welcome" -> {
                val hs = handshake ?: return fail("brak handshake'u")
                try {
                    val s = hs.finish(
                        v = msg.optInt("v", 0),
                        snB64 = msg.optString("sn", ""),
                        ts = msg.optLong("ts", 0),
                        epkB64 = msg.optString("epk", ""),
                        macB64 = msg.optString("mac", ""),
                    )
                    synchronized(sendLock) { session = s }
                    handshake = null
                    phase = Phase.ONLINE
                    attempt = 0
                    connectedSince = System.currentTimeMillis()
                    lastError = ""
                    Log.i(TAG, "Link polaczony ($endpoint)")
                    startPinger()
                    listener.onConnected()
                } catch (e: ClientHandshake.HandshakeError) {
                    fail("handshake: ${e.message}", if (e.reason == "clock") 60L else null)
                }
            }
            else -> fail("oczekiwano welcome")
        }
    }

    private fun onFrame(text: String) {
        val payload = try {
            val frame = JSONObject(text)
            if (frame.optString("t") != "e") return
            val s = session ?: return
            JSONObject(s.rx.decrypt(frame.getLong("n"), frame.getString("p")))
        } catch (e: Exception) {
            fail("uszkodzona ramka: ${e.message}")
            return
        }
        when (payload.optString("t")) {
            "pong" -> Unit
            "ping" -> send(JSONObject().put("t", "pong"))
            else -> try {
                listener.onMessage(payload)
            } catch (e: Exception) {
                Log.e(TAG, "obsluga wiadomosci", e)
            }
        }
    }

    private fun startPinger() {
        pinger?.cancel(false)
        pinger = loop.scheduleWithFixedDelay({
            if (phase != Phase.ONLINE) return@scheduleWithFixedDelay
            if (System.currentTimeMillis() - lastRx > LIVENESS_MS) {
                fail("brak odpowiedzi HA")
            } else {
                send(JSONObject().put("t", "ping"))
            }
        }, PING_S, PING_S, TimeUnit.SECONDS)
    }

    private inner class Callbacks(private val myGen: Int) : WebSocketListener() {
        private fun current(block: () -> Unit) = loop.execute { if (myGen == gen) block() }

        override fun onOpen(webSocket: WebSocket, response: Response) = current {
            val hs = handshake ?: return@current
            phase = Phase.HANDSHAKE
            webSocket.send(hs.helloJson())
            welcomeTimeout = loop.schedule({ if (myGen == gen && phase == Phase.HANDSHAKE) fail("HA nie odpowiedzial na hello") }, 10, TimeUnit.SECONDS)
        }

        override fun onMessage(webSocket: WebSocket, text: String) = current { onText(text) }

        override fun onClosing(webSocket: WebSocket, code: Int, reason: String) {
            webSocket.close(1000, null)
        }

        override fun onClosed(webSocket: WebSocket, code: Int, reason: String) = current {
            fail("polaczenie zamkniete ($code)")
        }

        override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) = current {
            val wasOnline = phase == Phase.ONLINE
            fail("blad sieci: ${t.javaClass.simpleName} ${t.message ?: ""}".trim(), rotate = !wasOnline)
        }
    }

    companion object {
        private const val TAG = "DeskmateTV"
        private val BACKOFF = longArrayOf(2, 4, 8, 15, 30, 60)
        private const val PING_S = 30L
        private const val LIVENESS_MS = 95_000L
    }
}
