package com.wawrzola.deskmatetv.ui

import android.app.Activity
import android.graphics.Color
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.util.TypedValue
import android.view.Gravity
import android.widget.Button
import android.widget.LinearLayout
import android.widget.TextView
import com.wawrzola.deskmatetv.Agent
import com.wawrzola.deskmatetv.R
import com.wawrzola.deskmatetv.service.ControlReceiver
import com.wawrzola.deskmatetv.service.TvAccessibilityService
import com.wawrzola.deskmatetv.service.Watchdog

/** Ekran diagnostyczny: stan polaczenia i uprawnien. Bez ustawien do klikania. */
class StatusActivity : Activity() {
    private val main = Handler(Looper.getMainLooper())
    private lateinit var body: TextView
    private val refresh = object : Runnable {
        override fun run() {
            render()
            main.postDelayed(this, 2000)
        }
    }

    private fun dp(v: Int) = TypedValue.applyDimension(TypedValue.COMPLEX_UNIT_DIP, v.toFloat(), resources.displayMetrics).toInt()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        Agent.init(this)
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(Color.rgb(16, 18, 22))
            setPadding(dp(48), dp(32), dp(48), dp(32))
        }
        root.addView(TextView(this).apply {
            text = "DeskmateTV"
            setTextColor(Color.WHITE)
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 30f)
        })
        root.addView(TextView(this).apply {
            text = getString(R.string.status_tagline)
            setTextColor(Color.rgb(170, 176, 186))
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 16f)
            setPadding(0, dp(4), 0, dp(20))
        })
        body = TextView(this).apply {
            setTextColor(Color.rgb(230, 233, 238))
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 18f)
            setLineSpacing(0f, 1.3f)
        }
        root.addView(body)
        val buttons = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.START
            setPadding(0, dp(24), 0, 0)
        }
        buttons.addView(Button(this).apply {
            text = getString(R.string.btn_reconnect)
            setOnClickListener {
                Thread { Watchdog.ensure(applicationContext) }.start()
                Agent.reconfigured(applicationContext)
            }
        })
        buttons.addView(Button(this).apply {
            text = getString(R.string.btn_test_bubble)
            setOnClickListener {
                TvAccessibilityService.instance?.overlay?.showBubble("DeskmateTV", getString(R.string.test_bubble_text), 6)
            }
        })
        root.addView(buttons)
        setContentView(root)
        buttons.getChildAt(0).requestFocus()
    }

    override fun onResume() {
        super.onResume()
        main.post(refresh)
    }

    override fun onPause() {
        main.removeCallbacks(refresh)
        super.onPause()
    }

    private fun yes(v: Boolean) = getString(if (v) R.string.yes else R.string.no)

    private fun render() {
        val s = try {
            ControlReceiver.status(this)
        } catch (e: Exception) {
            body.text = getString(R.string.st_error, e.message ?: "?")
            return
        }
        body.text = buildString {
            appendLine(getString(R.string.st_version, s.optString("version"), s.optInt("sdk"), s.optString("model")))
            appendLine(getString(R.string.st_node, s.optString("node")))
            appendLine(getString(R.string.st_paired, yes(s.optBoolean("configured"))))
            val link = if (s.optBoolean("connected")) getString(R.string.st_connected) else s.optString("phase")
            appendLine(getString(R.string.st_link, link, s.optString("endpoint")))
            val err = s.optString("last_error")
            if (err.isNotBlank()) appendLine(getString(R.string.st_last_error, err))
            appendLine(getString(R.string.st_a11y, yes(s.optBoolean("a11y"))))
            appendLine(getString(R.string.st_media, yes(s.optBoolean("media"))))
            appendLine(getString(R.string.st_watchdog, yes(s.optBoolean("write_secure"))))
            appendLine(getString(R.string.st_update, yes(s.optBoolean("install_allowed"))))
            appendLine(getString(R.string.st_caps, yes(s.optBoolean("dpad")), yes(s.optBoolean("screenshot"))))
            val upd = s.optJSONObject("update")?.optString("status").orEmpty()
            if (upd.isNotBlank()) appendLine(getString(R.string.st_last_update, upd))
        }
    }
}
