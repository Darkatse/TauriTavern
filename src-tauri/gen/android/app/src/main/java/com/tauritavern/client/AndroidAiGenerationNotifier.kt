package com.tauritavern.client

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.os.Build
import androidx.core.content.ContextCompat

class AndroidAiGenerationNotifier(
  private val context: Context,
) {
  fun ensureKeepAliveService() {
    ensureNotificationChannels(context)

    val intent = Intent(context, AiGenerationForegroundService::class.java).apply {
      action = AiGenerationForegroundService.ACTION_START
    }
    ContextCompat.startForegroundService(context, intent)
  }

  companion object {
    internal const val KEEPALIVE_CHANNEL_ID = "tauritavern_ai_generation_keepalive"

    internal fun ensureNotificationChannels(context: Context) {
      if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) {
        return
      }

      val notificationManager =
        context.getSystemService(Context.NOTIFICATION_SERVICE) as? NotificationManager ?: return

      val keepAliveChannel =
        NotificationChannel(
          KEEPALIVE_CHANNEL_ID,
          context.getString(R.string.notification_channel_ai_generation_name),
          NotificationManager.IMPORTANCE_LOW,
        ).apply {
          description = context.getString(R.string.notification_channel_ai_generation_description)
          setSound(null, null)
        }

      notificationManager.createNotificationChannel(keepAliveChannel)
    }

    internal fun buildLaunchIntent(context: Context): PendingIntent {
      val intent =
        Intent(context, MainActivity::class.java).apply {
          flags = Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP
        }

      return PendingIntent.getActivity(
        context,
        0,
        intent,
        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
      )
    }
  }
}
