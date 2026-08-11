package com.tauritavern.client

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.os.Build
import android.util.Log
import androidx.core.content.ContextCompat

class AndroidAiGenerationNotifier(
  private val context: Context,
) {
  fun onGenerationStart(taskId: String) {
    ContextCompat.startForegroundService(
      context,
      Intent(context, AiGenerationForegroundService::class.java).apply {
        action = AiGenerationForegroundService.ACTION_GENERATION_START
        putExtra(AiGenerationForegroundService.EXTRA_TASK_ID, taskId)
      },
    )
  }

  fun acknowledgeCompletionNotification() {
    val notificationManager =
      context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
    notificationManager.cancel(AiGenerationForegroundService.COMPLETION_NOTIFICATION_ID)
  }

  fun onGenerationProgress(outputTokens: Long) {
    try {
      context.startService(
        Intent(context, AiGenerationForegroundService::class.java).apply {
          action = AiGenerationForegroundService.ACTION_GENERATION_PROGRESS
          putExtra(AiGenerationForegroundService.EXTRA_OUTPUT_TOKENS, outputTokens)
        },
      )
    } catch (error: IllegalStateException) {
      Log.d(LOG_TAG, "Ignoring a background progress update after the service stopped", error)
    }
  }

  fun onGenerationFinish(
    taskId: String,
    outcome: String,
    statusCode: Int,
    notifyCompletion: Boolean,
  ) {
    context.startService(
      Intent(context, AiGenerationForegroundService::class.java).apply {
        action = AiGenerationForegroundService.ACTION_GENERATION_FINISH
        putExtra(AiGenerationForegroundService.EXTRA_TASK_ID, taskId)
        putExtra(AiGenerationForegroundService.EXTRA_OUTCOME, outcome)
        putExtra(AiGenerationForegroundService.EXTRA_STATUS_CODE, statusCode)
        putExtra(
          AiGenerationForegroundService.EXTRA_SHOW_COMPLETION_NOTIFICATION,
          notifyCompletion,
        )
      },
    )
  }

  companion object {
    private const val LOG_TAG = "TauriTavernAI"
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
