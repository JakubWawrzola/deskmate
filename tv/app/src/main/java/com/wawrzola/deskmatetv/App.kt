package com.wawrzola.deskmatetv

import android.app.Application
import com.wawrzola.deskmatetv.service.Watchdog

class App : Application() {
    override fun onCreate() {
        super.onCreate()
        Agent.init(this)
        Watchdog.schedule(this)
        Thread { Watchdog.ensure(this) }.start()
    }
}
