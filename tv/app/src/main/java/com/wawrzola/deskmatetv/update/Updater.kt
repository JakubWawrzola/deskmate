package com.wawrzola.deskmatetv.update

import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageInstaller
import android.content.pm.PackageManager
import android.os.Build
import android.util.Log
import android.view.accessibility.AccessibilityNodeInfo
import com.wawrzola.deskmatetv.service.TvAccessibilityService
import com.wawrzola.deskmatetv.tv.OpError
import com.wawrzola.deskmatetv.tv.ScreenReader
import org.json.JSONObject
import java.io.File
import java.io.RandomAccessFile
import java.security.MessageDigest
import java.util.Base64

/**
 * Samoaktualizacja przez Link (ADB po instalacji jest wylaczone).
 * HA wysyla APK fragmentami; przed instalacja sprawdzamy SHA-256 calosci,
 * nazwe pakietu, wyzsza wersje i ten sam certyfikat podpisu.
 */
object Updater {
    private const val TAG = "DeskmateTV"
    private const val MAX_SIZE = 64L * 1024 * 1024
    private val INSTALLER_PKGS = setOf("com.android.packageinstaller", "com.google.android.packageinstaller")
    private val CONFIRM_LABELS = listOf("zainstaluj", "aktualizuj", "install", "update")

    private var file: File? = null
    private var expected = 0L
    private var received = 0L
    private var sha = ""
    @Volatile private var autoConfirmUntil = 0L

    @Synchronized
    fun begin(ctx: Context, a: JSONObject): JSONObject {
        val size = a.optLong("size", -1)
        if (size !in 1..MAX_SIZE) throw OpError("bad_args", "zly rozmiar APK")
        val digest = a.optString("sha256").lowercase()
        if (!Regex("^[0-9a-f]{64}$").matches(digest)) throw OpError("bad_args", "zly sha256")
        val f = File(ctx.cacheDir, "update.apk")
        f.delete()
        RandomAccessFile(f, "rw").use { it.setLength(0) }
        file = f
        expected = size
        received = 0
        sha = digest
        return JSONObject().put("ready", true)
    }

    @Synchronized
    fun chunk(a: JSONObject): JSONObject {
        val f = file ?: throw OpError("bad_state", "brak update_begin")
        val offset = a.optLong("offset", -1)
        if (offset != received) throw OpError("bad_args", "oczekiwano offset $received, przyszedl $offset")
        val data = Base64.getDecoder().decode(a.optString("data"))
        if (received + data.size > expected) throw OpError("bad_args", "za duzo danych")
        RandomAccessFile(f, "rw").use {
            it.seek(offset)
            it.write(data)
        }
        received += data.size
        return JSONObject().put("received", received)
    }

    @Synchronized
    fun abort(): JSONObject {
        file?.delete()
        file = null
        received = 0
        return JSONObject()
    }

    @Synchronized
    fun end(ctx: Context, a: JSONObject): JSONObject {
        val f = file ?: throw OpError("bad_state", "brak update_begin")
        if (received != expected) throw OpError("bad_args", "niepelny plik: $received z $expected B")
        val actual = sha256(f)
        if (actual != sha) {
            abort()
            throw OpError("integrity", "SHA-256 sie nie zgadza")
        }
        val version = verify(ctx, f)
        install(ctx, f)
        file = null
        return JSONObject().put("status", "installing").put("version_code", version)
    }

    private fun sha256(f: File): String {
        val md = MessageDigest.getInstance("SHA-256")
        f.inputStream().use { input ->
            val buf = ByteArray(64 * 1024)
            while (true) {
                val n = input.read(buf)
                if (n <= 0) break
                md.update(buf, 0, n)
            }
        }
        return md.digest().joinToString("") { "%02x".format(it) }
    }

    /** Ten sam pakiet, wyzsza wersja, ten sam certyfikat. Zwraca nowa wersje. */
    private fun verify(ctx: Context, f: File): Long {
        val pm = ctx.packageManager
        val flags = PackageManager.GET_SIGNING_CERTIFICATES
        val archive = pm.getPackageArchiveInfo(f.path, flags) ?: throw OpError("integrity", "to nie jest poprawny APK")
        if (archive.packageName != ctx.packageName) throw OpError("integrity", "APK innej aplikacji: ${archive.packageName}")
        val current = pm.getPackageInfo(ctx.packageName, flags)
        if (archive.longVersionCode <= current.longVersionCode) {
            throw OpError("integrity", "wersja ${archive.longVersionCode} nie jest nowsza niz ${current.longVersionCode}")
        }
        fun certs(info: android.content.pm.PackageInfo): Set<String> =
            info.signingInfo?.apkContentsSigners.orEmpty().map { sig ->
                MessageDigest.getInstance("SHA-256").digest(sig.toByteArray()).joinToString("") { "%02x".format(it) }
            }.toSet()
        val mine = certs(current)
        if (mine.isEmpty() || certs(archive) != mine) throw OpError("integrity", "inny certyfikat podpisu")
        return archive.longVersionCode
    }

    private fun install(ctx: Context, f: File) {
        val installer = ctx.packageManager.packageInstaller
        val params = PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL)
        params.setAppPackageName(ctx.packageName)
        params.setSize(f.length())
        if (Build.VERSION.SDK_INT >= 31) {
            params.setRequireUserAction(PackageInstaller.SessionParams.USER_ACTION_NOT_REQUIRED)
        }
        val id = installer.createSession(params)
        installer.openSession(id).use { session ->
            session.openWrite("base.apk", 0, f.length()).use { out ->
                f.inputStream().use { it.copyTo(out) }
                session.fsync(out)
            }
            val intent = Intent(ctx, UpdateReceiver::class.java)
            var piFlags = PendingIntent.FLAG_UPDATE_CURRENT
            if (Build.VERSION.SDK_INT >= 31) piFlags = piFlags or PendingIntent.FLAG_MUTABLE
            val pi = PendingIntent.getBroadcast(ctx, id, intent, piFlags)
            setStatus(ctx, "instalacja w toku")
            session.commit(pi.intentSender)
        }
    }

    fun setStatus(ctx: Context, status: String) {
        ctx.getSharedPreferences("update", Context.MODE_PRIVATE).edit()
            .putString("status", status).putLong("at", System.currentTimeMillis() / 1000).apply()
    }

    fun lastStatus(ctx: Context): JSONObject {
        val p = ctx.getSharedPreferences("update", Context.MODE_PRIVATE)
        return JSONObject().put("status", p.getString("status", "") ?: "").put("at", p.getLong("at", 0))
    }

    fun armAutoConfirm() {
        autoConfirmUntil = System.currentTimeMillis() + 90_000
    }

    /**
     * Gdy system mimo wszystko pokaze okno potwierdzenia NASZEJ aktualizacji,
     * usluga dostepnosci klika "Zainstaluj". Dziala tylko 90 s po komendzie z HA.
     */
    fun onInstallerWindow(svc: TvAccessibilityService, pkg: String) {
        if (pkg !in INSTALLER_PKGS || System.currentTimeMillis() > autoConfirmUntil) return
        Thread {
            Thread.sleep(700)
            val root = svc.rootInActiveWindow ?: return@Thread
            for (label in CONFIRM_LABELS) {
                val hit = root.findAccessibilityNodeInfosByText(label).firstOrNull {
                    it.isEnabled && (it.isClickable || it.isFocusable) &&
                        ScreenReader.norm(it.text?.toString().orEmpty()) == label
                }
                if (hit != null) {
                    hit.performAction(AccessibilityNodeInfo.ACTION_CLICK)
                    Log.i(TAG, "aktualizacja: potwierdzono '$label'")
                    autoConfirmUntil = 0
                    return@Thread
                }
            }
        }.start()
    }
}

class UpdateReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val status = intent.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE)
        val message = intent.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE).orEmpty()
        when (status) {
            PackageInstaller.STATUS_PENDING_USER_ACTION -> {
                @Suppress("DEPRECATION")
                val confirm = if (Build.VERSION.SDK_INT >= 33) {
                    intent.getParcelableExtra(Intent.EXTRA_INTENT, Intent::class.java)
                } else {
                    intent.getParcelableExtra(Intent.EXTRA_INTENT)
                }
                if (confirm != null) {
                    Updater.armAutoConfirm()
                    confirm.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                    val starter: Context = TvAccessibilityService.instance ?: context
                    starter.startActivity(confirm)
                    Updater.setStatus(context, "czeka na potwierdzenie na TV")
                }
            }
            PackageInstaller.STATUS_SUCCESS -> Updater.setStatus(context, "zainstalowano")
            else -> Updater.setStatus(context, "blad instalacji ($status): $message")
        }
    }
}
