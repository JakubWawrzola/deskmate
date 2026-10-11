package com.wawrzola.deskmatetv.tv

import android.content.Context
import android.content.Intent
import android.media.tv.TvContract
import android.media.tv.TvInputInfo
import android.media.tv.TvInputManager
import org.json.JSONArray
import org.json.JSONObject

/**
 * Wejscia TV (HDMI, tuner). Lista z TvInputManager, przelaczenie przez
 * standardowy URI passthrough - obsluguje go systemowa aplikacja "TV".
 */
object Inputs {
    private fun manager(ctx: Context): TvInputManager? =
        ctx.getSystemService(Context.TV_INPUT_SERVICE) as? TvInputManager

    private fun typeName(type: Int): String = when (type) {
        TvInputInfo.TYPE_HDMI -> "hdmi"
        TvInputInfo.TYPE_TUNER -> "tuner"
        TvInputInfo.TYPE_COMPONENT -> "component"
        TvInputInfo.TYPE_COMPOSITE -> "composite"
        TvInputInfo.TYPE_DISPLAY_PORT -> "displayport"
        TvInputInfo.TYPE_DVI -> "dvi"
        TvInputInfo.TYPE_SCART -> "scart"
        TvInputInfo.TYPE_SVIDEO -> "svideo"
        TvInputInfo.TYPE_VGA -> "vga"
        else -> "other"
    }

    private fun stateName(state: Int): String = when (state) {
        TvInputManager.INPUT_STATE_CONNECTED -> "connected"
        TvInputManager.INPUT_STATE_CONNECTED_STANDBY -> "standby"
        TvInputManager.INPUT_STATE_DISCONNECTED -> "disconnected"
        else -> "unknown"
    }

    fun list(ctx: Context): JSONArray {
        val out = JSONArray()
        val tim = manager(ctx) ?: return out
        for (info in tim.tvInputList) {
            if (info.isHidden(ctx)) continue
            val custom = info.loadCustomLabel(ctx)?.toString()?.trim().orEmpty()
            val label = custom.ifEmpty { info.loadLabel(ctx)?.toString()?.trim().orEmpty() }
            val state = try {
                stateName(tim.getInputState(info.id))
            } catch (_: Exception) {
                "unknown"
            }
            out.put(
                JSONObject()
                    .put("id", info.id)
                    .put("label", label)
                    .put("type", typeName(info.type))
                    .put("state", state),
            )
        }
        return out
    }

    /** Intencja przelaczenia na wejscie; null gdy nie ma takiego wejscia. */
    fun switchIntent(ctx: Context, id: String): Intent? {
        val tim = manager(ctx) ?: return null
        if (tim.tvInputList.none { it.id == id }) return null
        return Intent(Intent.ACTION_VIEW, TvContract.buildChannelUriForPassthroughInput(id))
    }
}
