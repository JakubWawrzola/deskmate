package com.wawrzola.deskmatetv.service

import android.app.Activity
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import com.wawrzola.deskmatetv.Agent
import com.wawrzola.deskmatetv.Ops
import com.wawrzola.deskmatetv.link.LinkClient
import org.json.JSONObject

/**
 * Sterowanie z ADB (install.ps1). Odbiornik chroniony uprawnieniem
 * WRITE_SECURE_SETTINGS - ma je powloka ADB i sama aplikacja, zwykle
 * aplikacje nie. Wynik wraca w danych rozgloszenia (am broadcast go wypisuje).
 *
 *  PROVISION  --es code DMP1.... [--es node tv] [--es name DeskmateTV] [--es url ws://...] [--es assistant Jarvis]
 *  STATUS     -> JSON stanu
 *  RECONNECT  -> natychmiastowa proba polaczenia
 */
class ControlReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        Agent.init(context)
        val result = try {
            when (intent.action) {
                ACTION_PROVISION -> {
                    val summary = Agent.cfg.applyPairing(
                        code = intent.getStringExtra("code"),
                        psk = intent.getStringExtra("psk"),
                        node = intent.getStringExtra("node"),
                        name = intent.getStringExtra("name"),
                        url = intent.getStringExtra("url"),
                        cascade = intent.getStringExtra("cascade"),
                        assistant = intent.getStringExtra("assistant"),
                    )
                    Watchdog.ensure(context)
                    Agent.reconfigured(context)
                    JSONObject().put("ok", true).put("config", summary)
                }
                ACTION_STATUS -> status(context)
                ACTION_RECONNECT -> {
                    Agent.reconfigured(context)
                    JSONObject().put("ok", true)
                }
                else -> JSONObject().put("ok", false).put("error", "nieznana akcja")
            }
        } catch (e: Exception) {
            JSONObject().put("ok", false).put("error", e.message ?: e.javaClass.simpleName)
        }
        resultCode = if (result.optBoolean("ok")) Activity.RESULT_OK else Activity.RESULT_CANCELED
        resultData = result.toString()
    }

    companion object {
        const val ACTION_PROVISION = "com.wawrzola.deskmatetv.PROVISION"
        const val ACTION_STATUS = "com.wawrzola.deskmatetv.STATUS"
        const val ACTION_RECONNECT = "com.wawrzola.deskmatetv.RECONNECT"

        fun status(context: Context): JSONObject {
            val link = Agent.link
            return Ops.info(context)
                .put("ok", true)
                .put("configured", Agent.cfg.isConfigured)
                .put("connected", link.phase == LinkClient.Phase.ONLINE)
                .put("phase", link.phase.name)
                .put("endpoint", link.endpoint)
                .put("last_error", link.lastError)
        }
    }
}
