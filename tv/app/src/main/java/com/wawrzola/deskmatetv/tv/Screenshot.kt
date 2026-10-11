package com.wawrzola.deskmatetv.tv

import android.accessibilityservice.AccessibilityService
import android.graphics.Bitmap
import android.os.Build
import android.view.Display
import java.io.ByteArrayOutputStream
import java.util.Base64
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

/**
 * Zrzut obrazu ekranu - tylko na wyrazne zadanie Jarvisa (vision w HA).
 * Wideo z DRM wychodzi czarne; system pozwala na 1 zrzut na sekunde.
 */
object Screenshot {
    private val executor = Executors.newSingleThreadExecutor()

    fun take(svc: AccessibilityService, maxWidth: Int, quality: Int): Pair<String, IntArray> {
        if (Build.VERSION.SDK_INT < 30) throw OpError("unsupported", "zrzut ekranu wymaga Androida 11")
        val latch = CountDownLatch(1)
        var bitmap: Bitmap? = null
        var error: String? = null
        svc.takeScreenshot(Display.DEFAULT_DISPLAY, executor, object : AccessibilityService.TakeScreenshotCallback {
            override fun onSuccess(result: AccessibilityService.ScreenshotResult) {
                try {
                    val hw = Bitmap.wrapHardwareBuffer(result.hardwareBuffer, result.colorSpace)
                    bitmap = hw?.copy(Bitmap.Config.ARGB_8888, false)
                    hw?.recycle()
                } catch (e: Exception) {
                    error = e.message
                } finally {
                    result.hardwareBuffer.close()
                    latch.countDown()
                }
            }

            override fun onFailure(errorCode: Int) {
                error = "kod $errorCode"
                latch.countDown()
            }
        })
        if (!latch.await(6, TimeUnit.SECONDS)) throw OpError("timeout", "zrzut ekranu nie przyszedl")
        val full = bitmap ?: throw OpError("failed", "zrzut ekranu nieudany (${error ?: "?"})")
        val w = maxWidth.coerceIn(320, 1920)
        val scaled = if (full.width > w) {
            Bitmap.createScaledBitmap(full, w, (full.height.toLong() * w / full.width).toInt(), true)
        } else {
            full
        }
        val out = ByteArrayOutputStream()
        scaled.compress(Bitmap.CompressFormat.JPEG, quality.coerceIn(30, 90), out)
        val size = intArrayOf(scaled.width, scaled.height)
        if (scaled !== full) scaled.recycle()
        full.recycle()
        return Base64.getEncoder().encodeToString(out.toByteArray()) to size
    }
}
