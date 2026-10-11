package com.wawrzola.deskmatetv.service

import android.service.notification.NotificationListenerService
import com.wawrzola.deskmatetv.Agent
import com.wawrzola.deskmatetv.tv.Media

/**
 * Dostep do powiadomien jest potrzebny wylacznie po to, by system pokazal
 * aktywne sesje mediow (co gra). Tresci powiadomien nie czytamy ani nie wysylamy.
 */
class MediaListenerService : NotificationListenerService() {
    override fun onListenerConnected() {
        super.onListenerConnected()
        connected = true
        Media.attach(applicationContext)
        Agent.kick(applicationContext)
        Agent.refreshHealth()
    }

    override fun onListenerDisconnected() {
        connected = false
        Media.detach(applicationContext)
        Agent.refreshHealth()
        super.onListenerDisconnected()
    }

    companion object {
        @Volatile var connected: Boolean = false
            private set
    }
}
