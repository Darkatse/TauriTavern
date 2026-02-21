package com.tauritavern.client

import android.app.Notification
import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import androidx.core.app.NotificationCompat

class AiGenerationForegroundService : Service() {
  override fun onBind(intent: Intent?): IBinder? = null

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    startForegroundSafely()
    // Ask Android to recreate this service when possible after process reclaim.
    return START_STICKY
  }

  override fun onDestroy() {
    stopForegroundCompat()
    super.onDestroy()
  }

  private fun startForegroundSafely() {
    val notification = buildKeepAliveNotification()
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
      startForeground(
        KEEPALIVE_NOTIFICATION_ID,
        notification,
        ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC,
      )
      return
    }

    startForeground(KEEPALIVE_NOTIFICATION_ID, notification)
  }

  private fun stopForegroundCompat() {
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.N) {
      stopForeground(STOP_FOREGROUND_REMOVE)
      return
    }

    @Suppress("DEPRECATION")
    stopForeground(true)
  }

  private fun buildKeepAliveNotification(): Notification {
    AndroidAiGenerationNotifier.ensureNotificationChannels(this)

    return NotificationCompat.Builder(this, AndroidAiGenerationNotifier.KEEPALIVE_CHANNEL_ID)
      .setSmallIcon(R.mipmap.ic_launcher)
      .setContentTitle(getString(R.string.notification_ai_keepalive_title))
      .setContentText(getString(R.string.notification_ai_keepalive_body))
      .setCategory(NotificationCompat.CATEGORY_SERVICE)
      .setPriority(NotificationCompat.PRIORITY_LOW)
      .setOnlyAlertOnce(true)
      .setOngoing(true)
      .setContentIntent(AndroidAiGenerationNotifier.buildLaunchIntent(this))
      .build()
  }

  companion object {
    const val ACTION_START = "com.tauritavern.client.action.AI_GENERATION_START"
    const val KEEPALIVE_NOTIFICATION_ID = 42000
  }
}
