package com.tauritavern.client

import android.app.Notification
import android.app.NotificationManager
import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.Bundle
import android.os.IBinder
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationCompat.ProgressStyle
import androidx.core.graphics.drawable.IconCompat

class AiGenerationForegroundService : Service() {
  private val notificationManager: NotificationManager by lazy {
    getSystemService(NOTIFICATION_SERVICE) as NotificationManager
  }

  private var startedAtMs: Long = 0L
  private var outputTokens: Long = 0L
  private var isGenerating: Boolean = false

  override fun onBind(intent: Intent?): IBinder? = null

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    AndroidAiGenerationNotifier.ensureNotificationChannels(this)

    when (intent?.action) {
      null, ACTION_ENSURE_KEEPALIVE -> ensureKeepAlive()
      ACTION_GENERATION_START -> handleGenerationStart()
      ACTION_GENERATION_PROGRESS -> handleGenerationProgress(requireNotNull(intent.extras))
      ACTION_GENERATION_FINISH -> handleGenerationFinish(requireNotNull(intent.extras))
      ACTION_GENERATION_STOP -> handleGenerationStop()
      else -> error("Unknown intent action: ${intent.action}")
    }

    return START_STICKY
  }

  override fun onDestroy() {
    stopForeground(STOP_FOREGROUND_REMOVE)
    super.onDestroy()
  }

  private fun supportsLiveUpdates(): Boolean {
    return Build.VERSION.SDK_INT >= Build.VERSION_CODES.BAKLAVA
  }

  private fun ensureKeepAlive() {
    isGenerating = false
    startedAtMs = 0L
    outputTokens = 0L

    startForegroundCompat(buildKeepAliveNotification())
  }

  private fun handleGenerationStart() {
    isGenerating = true
    startedAtMs = System.currentTimeMillis()
    outputTokens = 0L

    if (!supportsLiveUpdates()) {
      startForegroundCompat(buildKeepAliveNotification())
      return
    }

    startForegroundCompat(buildLiveUpdateGeneratingNotification())
  }

  private fun handleGenerationProgress(extras: Bundle) {
    check(isGenerating) { "AI generation progress received before start" }
    check(extras.containsKey(EXTRA_OUTPUT_TOKENS)) { "Missing output token count extra" }

    outputTokens = extras.getLong(EXTRA_OUTPUT_TOKENS)

    if (!supportsLiveUpdates() || !isGenerating) {
      return
    }

    notificationManager.notify(NOTIFICATION_ID, buildLiveUpdateGeneratingNotification())
  }

  private fun handleGenerationFinish(extras: Bundle) {
    check(extras.containsKey(EXTRA_SUCCESS)) { "Missing success extra" }
    check(extras.containsKey(EXTRA_STATUS_CODE)) { "Missing status code extra" }
    check(extras.containsKey(EXTRA_SHOW_COMPLETION_NOTIFICATION)) {
      "Missing show completion notification extra"
    }

    val success = extras.getBoolean(EXTRA_SUCCESS)
    val statusCode = extras.getInt(EXTRA_STATUS_CODE)
    val showCompletionNotification = extras.getBoolean(EXTRA_SHOW_COMPLETION_NOTIFICATION)

    isGenerating = false

    if (!supportsLiveUpdates()) {
      startForegroundCompat(buildKeepAliveNotification())
      return
    }

    if (showCompletionNotification) {
      notificationManager.notify(
        COMPLETION_NOTIFICATION_ID,
        if (success) {
          buildLiveUpdateSuccessNotification()
        } else {
          buildLiveUpdateFailureNotification(statusCode)
        },
      )
    }

    startForegroundCompat(buildKeepAliveNotification())
  }

  private fun handleGenerationStop() {
    isGenerating = false
    startedAtMs = 0L
    outputTokens = 0L

    startForegroundCompat(buildKeepAliveNotification())
  }

  private fun startForegroundCompat(notification: Notification) {
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
      startForeground(
        NOTIFICATION_ID,
        notification,
        ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC,
      )
      return
    }

    startForeground(NOTIFICATION_ID, notification)
  }

  private fun buildKeepAliveNotification(): Notification {
    return NotificationCompat.Builder(this, AndroidAiGenerationNotifier.KEEPALIVE_CHANNEL_ID)
      .setSmallIcon(R.mipmap.ic_launcher)
      .setContentTitle(getString(R.string.notification_ai_keepalive_title))
      .setContentText(getString(R.string.notification_ai_keepalive_body))
      .setCategory(NotificationCompat.CATEGORY_SERVICE)
      .setPriority(NotificationCompat.PRIORITY_LOW)
      .setOnlyAlertOnce(true)
      .setSilent(true)
      .setOngoing(true)
      .setContentIntent(AndroidAiGenerationNotifier.buildLaunchIntent(this))
      .build()
  }

  private fun buildLiveUpdateGeneratingNotification(): Notification {
    val pointColor = 0xFFECB7FF.toInt()
    val segmentColor = 0xFF86F7FA.toInt()

    val progressStyle =
      ProgressStyle()
        .setProgressIndeterminate(true)
        .setProgressPoints(
          listOf(
            ProgressStyle.Point(25).setColor(pointColor),
            ProgressStyle.Point(50).setColor(pointColor),
            ProgressStyle.Point(75).setColor(pointColor),
            ProgressStyle.Point(100).setColor(pointColor),
          ),
        )
        .setProgressSegments(
          listOf(
            ProgressStyle.Segment(25).setColor(segmentColor),
            ProgressStyle.Segment(25).setColor(segmentColor),
            ProgressStyle.Segment(25).setColor(segmentColor),
            ProgressStyle.Segment(25).setColor(segmentColor),
          ),
        )
        .setProgressTrackerIcon(IconCompat.createWithResource(this, R.drawable.ic_launcher_foreground))

    val title = getString(R.string.notification_ai_live_title)
    val body = getString(R.string.notification_ai_live_body, outputTokens)

    return NotificationCompat.Builder(this, AndroidAiGenerationNotifier.LIVE_UPDATE_CHANNEL_ID)
      .setSmallIcon(R.mipmap.ic_launcher)
      .setContentTitle(title)
      .setContentText(body)
      .setShortCriticalText(getString(R.string.notification_ai_live_short))
      .setStyle(progressStyle)
      .setCategory(NotificationCompat.CATEGORY_SERVICE)
      .setPriority(NotificationCompat.PRIORITY_DEFAULT)
      .setOnlyAlertOnce(true)
      .setSilent(true)
      .setOngoing(true)
      .setRequestPromotedOngoing(true)
      .setForegroundServiceBehavior(NotificationCompat.FOREGROUND_SERVICE_IMMEDIATE)
      .setWhen(startedAtMs)
      .setUsesChronometer(true)
      .setContentIntent(AndroidAiGenerationNotifier.buildLaunchIntent(this))
      .build()
  }

  private fun buildLiveUpdateSuccessNotification(): Notification {
    val pointColor = 0xFFECB7FF.toInt()
    val segmentColor = 0xFF86F7FA.toInt()

    val progressStyle =
      ProgressStyle()
        .setProgressTrackerIcon(IconCompat.createWithResource(this, R.drawable.ic_launcher_foreground))
        .setProgressPoints(
          listOf(
            ProgressStyle.Point(25).setColor(pointColor),
            ProgressStyle.Point(50).setColor(pointColor),
            ProgressStyle.Point(75).setColor(pointColor),
            ProgressStyle.Point(100).setColor(pointColor),
          ),
        )
        .setProgressSegments(
          listOf(
            ProgressStyle.Segment(25).setColor(segmentColor),
            ProgressStyle.Segment(25).setColor(segmentColor),
            ProgressStyle.Segment(25).setColor(segmentColor),
            ProgressStyle.Segment(25).setColor(segmentColor),
          ),
        )
        .setProgress(100)

    return NotificationCompat.Builder(this, AndroidAiGenerationNotifier.LIVE_UPDATE_CHANNEL_ID)
      .setSmallIcon(R.mipmap.ic_launcher)
      .setContentTitle(getString(R.string.notification_ai_done_title))
      .setContentText(getString(R.string.notification_ai_done_body))
      .setShortCriticalText(getString(R.string.notification_ai_done_short))
      .setStyle(progressStyle)
      .setCategory(NotificationCompat.CATEGORY_STATUS)
      .setPriority(NotificationCompat.PRIORITY_DEFAULT)
      .setOnlyAlertOnce(true)
      .setAutoCancel(true)
      .setContentIntent(AndroidAiGenerationNotifier.buildLaunchIntent(this))
      .build()
  }

  private fun buildLiveUpdateFailureNotification(statusCode: Int): Notification {
    val title = getString(R.string.notification_ai_failed_title)
    val body =
      if (statusCode > 0) {
        getString(R.string.notification_ai_failed_body_with_code, statusCode)
      } else {
        getString(R.string.notification_ai_failed_body)
      }

    val shortText =
      if (statusCode > 0) {
        statusCode.toString()
      } else {
        getString(R.string.notification_ai_failed_short)
      }

    return NotificationCompat.Builder(this, AndroidAiGenerationNotifier.LIVE_UPDATE_CHANNEL_ID)
      .setSmallIcon(R.mipmap.ic_launcher)
      .setContentTitle(title)
      .setContentText(body)
      .setShortCriticalText(shortText)
      .setCategory(NotificationCompat.CATEGORY_ERROR)
      .setPriority(NotificationCompat.PRIORITY_DEFAULT)
      .setOnlyAlertOnce(true)
      .setAutoCancel(true)
      .setContentIntent(AndroidAiGenerationNotifier.buildLaunchIntent(this))
      .build()
  }

  companion object {
    const val ACTION_ENSURE_KEEPALIVE = "com.tauritavern.client.action.AI_KEEPALIVE_ENSURE"
    const val ACTION_GENERATION_START = "com.tauritavern.client.action.AI_GENERATION_START"
    const val ACTION_GENERATION_PROGRESS = "com.tauritavern.client.action.AI_GENERATION_PROGRESS"
    const val ACTION_GENERATION_FINISH = "com.tauritavern.client.action.AI_GENERATION_FINISH"
    const val ACTION_GENERATION_STOP = "com.tauritavern.client.action.AI_GENERATION_STOP"

    const val EXTRA_OUTPUT_TOKENS = "com.tauritavern.client.extra.OUTPUT_TOKENS"
    const val EXTRA_SUCCESS = "com.tauritavern.client.extra.SUCCESS"
    const val EXTRA_STATUS_CODE = "com.tauritavern.client.extra.STATUS_CODE"
    const val EXTRA_SHOW_COMPLETION_NOTIFICATION =
      "com.tauritavern.client.extra.SHOW_COMPLETION_NOTIFICATION"

    const val NOTIFICATION_ID = 42000
    const val COMPLETION_NOTIFICATION_ID = 42001
  }
}
