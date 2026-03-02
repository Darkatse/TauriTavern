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
    ContextCompat.startForegroundService(
      context,
      Intent(context, AiGenerationForegroundService::class.java).apply {
        action = AiGenerationForegroundService.ACTION_ENSURE_KEEPALIVE
      },
    )
  }

  fun onGenerationStart() {
    ContextCompat.startForegroundService(
      context,
      Intent(context, AiGenerationForegroundService::class.java).apply {
        action = AiGenerationForegroundService.ACTION_GENERATION_START
      },
    )
  }

  fun onGenerationProgress(outputTokens: Long) {
    ContextCompat.startForegroundService(
      context,
      Intent(context, AiGenerationForegroundService::class.java).apply {
        action = AiGenerationForegroundService.ACTION_GENERATION_PROGRESS
        putExtra(AiGenerationForegroundService.EXTRA_OUTPUT_TOKENS, outputTokens)
      },
    )
  }

  fun onGenerationFinish(
    success: Boolean,
    statusCode: Int,
    showCompletionNotification: Boolean,
  ) {
    ContextCompat.startForegroundService(
      context,
      Intent(context, AiGenerationForegroundService::class.java).apply {
        action = AiGenerationForegroundService.ACTION_GENERATION_FINISH
        putExtra(AiGenerationForegroundService.EXTRA_SUCCESS, success)
        putExtra(AiGenerationForegroundService.EXTRA_STATUS_CODE, statusCode)
        putExtra(
          AiGenerationForegroundService.EXTRA_SHOW_COMPLETION_NOTIFICATION,
          showCompletionNotification,
        )
      },
    )
  }

  fun onGenerationStop() {
    ContextCompat.startForegroundService(
      context,
      Intent(context, AiGenerationForegroundService::class.java).apply {
        action = AiGenerationForegroundService.ACTION_GENERATION_STOP
      },
    )
  }

  companion object {
    internal const val KEEPALIVE_CHANNEL_ID = "tauritavern_ai_generation_keepalive"
    internal const val LIVE_UPDATE_CHANNEL_ID = "tauritavern_ai_generation_live_updates"

    internal fun ensureNotificationChannels(context: Context) {
      if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) {
        return
      }

      val notificationManager =
        context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager

      val keepAliveChannel =
        NotificationChannel(
          KEEPALIVE_CHANNEL_ID,
          context.getString(R.string.notification_channel_ai_generation_name),
          NotificationManager.IMPORTANCE_LOW,
        ).apply {
          description = context.getString(R.string.notification_channel_ai_generation_description)
          setSound(null, null)
          enableVibration(false)
          setShowBadge(false)
        }

      val liveUpdateChannel =
        NotificationChannel(
          LIVE_UPDATE_CHANNEL_ID,
          context.getString(R.string.notification_channel_ai_live_updates_name),
          NotificationManager.IMPORTANCE_DEFAULT,
        ).apply {
          description =
            context.getString(R.string.notification_channel_ai_live_updates_description)
          setSound(null, null)
          enableVibration(false)
          setShowBadge(false)
        }

      notificationManager.createNotificationChannel(keepAliveChannel)
      notificationManager.createNotificationChannel(liveUpdateChannel)
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
