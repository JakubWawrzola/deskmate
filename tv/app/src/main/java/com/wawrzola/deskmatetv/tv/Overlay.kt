package com.wawrzola.deskmatetv.tv

import android.accessibilityservice.AccessibilityService
import android.graphics.Bitmap
import android.graphics.Color
import android.graphics.PixelFormat
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.os.Handler
import android.os.Looper
import android.util.TypedValue
import android.view.Gravity
import android.view.View
import android.view.WindowManager
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.TextView
import com.wawrzola.deskmatetv.Agent
import com.wawrzola.deskmatetv.R

/**
 * Nakladka na ekranie TV (okno TYPE_ACCESSIBILITY_OVERLAY - bez uprawnienia
 * "wyswietlaj nad innymi"). Nie przejmuje fokusu ani dotyku, wiec D-pad
 * zostaje w aplikacji. Dymek: prawy dol. Wskaznik "Jarvis steruje": prawa gora.
 */
class Overlay(private val svc: AccessibilityService) {
    private val main = Handler(Looper.getMainLooper())
    private val wm = svc.getSystemService(WindowManager::class.java)
    private var bubble: View? = null
    private var indicator: View? = null
    private val hideBubble = Runnable { remove(bubble); bubble = null }
    private val hideIndicator = Runnable { remove(indicator); indicator = null }

    private fun dp(v: Int): Int = TypedValue.applyDimension(
        TypedValue.COMPLEX_UNIT_DIP, v.toFloat(), svc.resources.displayMetrics,
    ).toInt()

    private fun params(gravity: Int, x: Int, y: Int, maxWidth: Int? = null) =
        WindowManager.LayoutParams(
            maxWidth ?: WindowManager.LayoutParams.WRAP_CONTENT,
            WindowManager.LayoutParams.WRAP_CONTENT,
            WindowManager.LayoutParams.TYPE_ACCESSIBILITY_OVERLAY,
            WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE or
                WindowManager.LayoutParams.FLAG_NOT_TOUCHABLE or
                WindowManager.LayoutParams.FLAG_LAYOUT_IN_SCREEN,
            PixelFormat.TRANSLUCENT,
        ).apply {
            this.gravity = gravity
            this.x = x
            this.y = y
        }

    private fun card(radius: Int) = GradientDrawable().apply {
        setColor(Color.argb(235, 18, 20, 24))
        cornerRadius = dp(radius).toFloat()
        setStroke(dp(1), Color.argb(70, 255, 255, 255))
    }

    /** Dymek z tytulem, trescia i opcjonalnym obrazem (np. kamera), znika po [seconds]. */
    fun showBubble(title: String, text: String, seconds: Int, image: Bitmap? = null) = main.post {
        remove(bubble)
        val box = LinearLayout(svc).apply {
            orientation = LinearLayout.VERTICAL
            background = card(14)
            setPadding(dp(20), dp(14), dp(20), dp(16))
        }
        if (image != null) {
            box.addView(ImageView(svc).apply {
                setImageBitmap(image)
                adjustViewBounds = true
                scaleType = ImageView.ScaleType.FIT_CENTER
                maxHeight = (svc.resources.displayMetrics.heightPixels * 0.42f).toInt()
                layoutParams = LinearLayout.LayoutParams(
                    LinearLayout.LayoutParams.MATCH_PARENT,
                    LinearLayout.LayoutParams.WRAP_CONTENT,
                ).apply { bottomMargin = dp(10) }
            })
        }
        if (title.isNotBlank()) {
            box.addView(TextView(svc).apply {
                this.text = title
                setTextColor(Color.WHITE)
                setTextSize(TypedValue.COMPLEX_UNIT_SP, 20f)
                typeface = Typeface.DEFAULT_BOLD
            })
        }
        if (text.isNotBlank()) {
            box.addView(TextView(svc).apply {
                this.text = text
                setTextColor(Color.argb(225, 235, 238, 242))
                setTextSize(TypedValue.COMPLEX_UNIT_SP, 17f)
                setLineSpacing(0f, 1.15f)
                maxLines = 8
            })
        }
        val width = (svc.resources.displayMetrics.widthPixels * 0.34f).toInt()
        try {
            wm.addView(box, params(Gravity.BOTTOM or Gravity.END, dp(36), dp(36), width))
            bubble = box
            main.removeCallbacks(hideBubble)
            main.postDelayed(hideBubble, seconds.coerceIn(2, 60) * 1000L)
        } catch (_: Exception) {
            bubble = null
        }
    }

    /** Pokazuje lub przedluza wskaznik "Jarvis steruje" (znika po 6 s ciszy). */
    fun pulseIndicator() = main.post {
        if (indicator == null) {
            val pill = LinearLayout(svc).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER_VERTICAL
                background = card(20)
                setPadding(dp(14), dp(8), dp(16), dp(8))
            }
            pill.addView(View(svc).apply {
                background = GradientDrawable().apply {
                    shape = GradientDrawable.OVAL
                    setColor(Color.rgb(64, 196, 255))
                }
                layoutParams = LinearLayout.LayoutParams(dp(10), dp(10)).apply { marginEnd = dp(10) }
            })
            pill.addView(TextView(svc).apply {
                text = svc.getString(R.string.indicator, Agent.cfg.assistantName)
                setTextColor(Color.WHITE)
                setTextSize(TypedValue.COMPLEX_UNIT_SP, 15f)
            })
            try {
                wm.addView(pill, params(Gravity.TOP or Gravity.END, dp(28), dp(24)))
                indicator = pill
            } catch (_: Exception) {
                indicator = null
            }
        }
        main.removeCallbacks(hideIndicator)
        main.postDelayed(hideIndicator, 6000)
    }

    fun removeAll() = main.post {
        main.removeCallbacks(hideBubble)
        main.removeCallbacks(hideIndicator)
        remove(bubble)
        remove(indicator)
        bubble = null
        indicator = null
    }

    private fun remove(view: View?) {
        if (view == null) return
        try {
            wm.removeViewImmediate(view)
        } catch (_: Exception) {
        }
    }
}
