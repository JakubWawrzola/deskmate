package com.wawrzola.deskmatetv.tv

import android.content.ComponentName
import android.content.Context
import android.media.AudioManager
import android.media.MediaMetadata
import android.media.session.MediaController
import android.media.session.MediaSessionManager
import android.media.session.PlaybackState
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import com.wawrzola.deskmatetv.Agent
import com.wawrzola.deskmatetv.service.MediaListenerService
import org.json.JSONObject

/**
 * Co gra w dowolnej aplikacji (MediaSession) i sterowanie odtwarzaniem.
 * Wymaga wlaczonego dostepu do powiadomien (MediaListenerService).
 */
object Media {
    private val main = Handler(Looper.getMainLooper())
    private val bound = ArrayList<Pair<MediaController, MediaController.Callback>>()
    private var sessionsListener: MediaSessionManager.OnActiveSessionsChangedListener? = null

    private fun component(ctx: Context) = ComponentName(ctx, MediaListenerService::class.java)

    fun controllers(ctx: Context): List<MediaController> = try {
        val msm = ctx.getSystemService(MediaSessionManager::class.java)
        msm.getActiveSessions(component(ctx))
    } catch (_: SecurityException) {
        emptyList()
    }

    fun available(ctx: Context): Boolean = try {
        ctx.getSystemService(MediaSessionManager::class.java).getActiveSessions(component(ctx))
        true
    } catch (_: SecurityException) {
        false
    }

    fun current(ctx: Context): MediaController? {
        val list = controllers(ctx)
        return list.firstOrNull { it.playbackState?.state == PlaybackState.STATE_PLAYING } ?: list.firstOrNull()
    }

    fun stateName(state: Int?): String = when (state) {
        PlaybackState.STATE_PLAYING -> "playing"
        PlaybackState.STATE_PAUSED -> "paused"
        PlaybackState.STATE_BUFFERING, PlaybackState.STATE_CONNECTING -> "buffering"
        PlaybackState.STATE_STOPPED, PlaybackState.STATE_NONE, null -> "stopped"
        PlaybackState.STATE_ERROR -> "error"
        PlaybackState.STATE_FAST_FORWARDING -> "ff"
        PlaybackState.STATE_REWINDING -> "rw"
        else -> "other"
    }

    private fun position(ps: PlaybackState?): Long {
        if (ps == null) return 0
        var pos = ps.position
        if (ps.state == PlaybackState.STATE_PLAYING && ps.lastPositionUpdateTime > 0) {
            pos += ((SystemClock.elapsedRealtime() - ps.lastPositionUpdateTime) * ps.playbackSpeed).toLong()
        }
        return pos.coerceAtLeast(0)
    }

    fun title(c: MediaController): String {
        val md = c.metadata ?: return ""
        return md.getString(MediaMetadata.METADATA_KEY_DISPLAY_TITLE)
            ?: md.getString(MediaMetadata.METADATA_KEY_TITLE)
            ?: ""
    }

    fun describe(ctx: Context, c: MediaController?): JSONObject {
        if (c == null) return JSONObject().put("state", "none").put("uwaga", "brak aktywnej sesji mediow")
        val md = c.metadata
        val ps = c.playbackState
        val out = JSONObject()
            .put("pkg", c.packageName)
            .put("app", Apps.label(ctx, c.packageName))
            .put("title", title(c))
            .put("state", stateName(ps?.state))
            .put("pos_s", position(ps) / 1000)
        md?.getString(MediaMetadata.METADATA_KEY_DISPLAY_SUBTITLE)?.let { out.put("subtitle", it) }
            ?: md?.getString(MediaMetadata.METADATA_KEY_ARTIST)?.let { out.put("subtitle", it) }
        md?.getLong(MediaMetadata.METADATA_KEY_DURATION)?.takeIf { it > 0 }?.let { out.put("dur_s", it / 1000) }
        return out
    }

    fun control(ctx: Context, action: String, posMs: Long?, seconds: Long?): JSONObject {
        val c = current(ctx) ?: throw OpError("no_media", "brak aktywnej sesji mediow")
        val t = c.transportControls
        val step = (seconds ?: 10L) * 1000
        when (action) {
            "play" -> t.play()
            "pause" -> t.pause()
            "toggle" -> if (c.playbackState?.state == PlaybackState.STATE_PLAYING) t.pause() else t.play()
            "stop" -> t.stop()
            "next" -> t.skipToNext()
            "prev" -> t.skipToPrevious()
            "seek" -> t.seekTo(posMs ?: throw OpError("bad_args", "seek wymaga pos (ms)"))
            "ff" -> t.seekTo(position(c.playbackState) + step)
            "rw" -> t.seekTo((position(c.playbackState) - step).coerceAtLeast(0))
            "" , "state" -> Unit
            else -> throw OpError("bad_args", "nieznana akcja media: $action")
        }
        return describe(ctx, c)
    }

    // ── sledzenie zmian (push do HA) ─────────────────────────────

    fun attach(ctx: Context) = main.post {
        val msm = ctx.getSystemService(MediaSessionManager::class.java)
        if (sessionsListener == null) {
            val listener = MediaSessionManager.OnActiveSessionsChangedListener { list -> rebind(ctx, list ?: emptyList()) }
            try {
                msm.addOnActiveSessionsChangedListener(listener, component(ctx), main)
                sessionsListener = listener
            } catch (_: SecurityException) {
                return@post
            }
        }
        rebind(ctx, controllers(ctx))
    }

    fun detach(ctx: Context) = main.post {
        sessionsListener?.let {
            ctx.getSystemService(MediaSessionManager::class.java).removeOnActiveSessionsChangedListener(it)
        }
        sessionsListener = null
        unbindAll()
    }

    private fun unbindAll() {
        for ((c, cb) in bound) {
            try {
                c.unregisterCallback(cb)
            } catch (_: Exception) {
            }
        }
        bound.clear()
    }

    private fun rebind(ctx: Context, list: List<MediaController>) {
        unbindAll()
        for (c in list) {
            val cb = object : MediaController.Callback() {
                override fun onPlaybackStateChanged(state: PlaybackState?) = publish(ctx)
                override fun onMetadataChanged(metadata: MediaMetadata?) = publish(ctx)
                override fun onSessionDestroyed() = publish(ctx)
            }
            c.registerCallback(cb, main)
            bound += c to cb
        }
        publish(ctx)
    }

    private val publishRunnable = Runnable { pending?.let { Agent.onMedia(it) } }
    @Volatile private var pending: Context? = null

    private fun publish(ctx: Context) {
        pending = ctx
        main.removeCallbacks(publishRunnable)
        main.postDelayed(publishRunnable, 600)
    }
}

/** Glosnosc w procentach (STREAM_MUSIC; z glosnikiem BT ustawia glosnik). */
object Volume {
    private fun am(ctx: Context) = ctx.getSystemService(AudioManager::class.java)

    fun get(ctx: Context): JSONObject {
        val am = am(ctx)
        val max = am.getStreamMaxVolume(AudioManager.STREAM_MUSIC).coerceAtLeast(1)
        val cur = am.getStreamVolume(AudioManager.STREAM_MUSIC)
        return JSONObject()
            .put("level", Math.round(cur * 100f / max))
            .put("muted", am.isStreamMute(AudioManager.STREAM_MUSIC))
            .put("fixed", am.isVolumeFixed)
    }

    fun apply(ctx: Context, set: Int?, delta: Int?, mute: Boolean?): JSONObject {
        val am = am(ctx)
        if (am.isVolumeFixed) throw OpError("unsupported", "glosnosc stala w tym trybie audio - uzyj pilota")
        val max = am.getStreamMaxVolume(AudioManager.STREAM_MUSIC).coerceAtLeast(1)
        if (set != null) {
            val idx = Math.round(set.coerceIn(0, 100) * max / 100f)
            am.setStreamVolume(AudioManager.STREAM_MUSIC, idx, 0)
        }
        if (delta != null && delta != 0) {
            val cur = am.getStreamVolume(AudioManager.STREAM_MUSIC)
            val step = Math.round(delta * max / 100f).let { if (it == 0) (if (delta > 0) 1 else -1) else it }
            am.setStreamVolume(AudioManager.STREAM_MUSIC, (cur + step).coerceIn(0, max), 0)
        }
        if (mute != null) {
            am.adjustStreamVolume(
                AudioManager.STREAM_MUSIC,
                if (mute) AudioManager.ADJUST_MUTE else AudioManager.ADJUST_UNMUTE,
                0,
            )
        }
        return get(ctx)
    }
}

/** Blad operacji z kodem dla HA (np. confirm, stale, unsupported). */
class OpError(val code: String, message: String, val extra: JSONObject? = null) : Exception(message)
