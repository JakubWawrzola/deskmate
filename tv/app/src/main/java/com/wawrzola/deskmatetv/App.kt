package com.wawrzola.deskmatetv

import android.app.Application
import com.wawrzola.deskmatetv.service.Watchdog
import com.wawrzola.deskmatetv.update.Updater

class App : Application() {
    override fun onCreate() {
        super.onCreate()
        Agent.init(this)
        Watchdog.schedule(this)
        Updater.abandonStaleSessions(this)
        Thread { Watchdog.ensure(this) }.start()
    }
}
