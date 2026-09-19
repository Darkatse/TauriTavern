package com.tauritavern.client

import android.app.Notification
import android.app.NotificationManager
import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.Bundle
import android.os.IBinder
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationCompat.ProgressStyle
import androidx.core.graphics.drawable.IconCompat

class AiGenerationForegroundService : Service() {
  private val notificationManager: NotificationManager by lazy {
    getSystemService(NOTIFICATION_SERVICE) as NotificationManager
  }

  private var startedAtMs: Long = 0L
  private var outputTokens: Long = 0L
  private val activeTaskIds = mutableSetOf<String>()

  override fun onBind(intent: Intent?): IBinder? = null

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    AndroidAiGenerationNotifier.ensureNotificationChannels(this)

    when (intent?.action) {
      null -> stopSelf(startId)
      ACTION_GENERATION_START -> handleGenerationStart(requireNotNull(intent.extras))
      ACTION_GENERATION_PROGRESS ->
        handleGenerationProgress(requireNotNull(intent.extras), startId)
      ACTION_GENERATION_FINISH ->
        handleGenerationFinish(requireNotNull(intent.extras), startId)
      else -> error("Unknown intent action: ${intent.action}")
    }

    return START_NOT_STICKY
  }

  override fun onDestroy() {
    activeTaskIds.clear()
    stopForeground(STOP_FOREGROUND_REMOVE)
    super.onDestroy()
  }

  override fun onTimeout(startId: Int, fgsType: Int) {
    Log.w(LOG_TAG, "AI generation foreground service reached the Android time limit")
    activeTaskIds.clear()
    stopForegroundAndSelf(startId)
  }

  private fun supportsLiveUpdates(): Boolean {
    return Build.VERSION.SDK_INT >= Build.VERSION_CODES.BAKLAVA
  }

  private fun handleGenerationStart(extras: Bundle) {
    val taskId = requireTaskId(extras)
    if (activeTaskIds.isEmpty()) {
      startedAtMs = System.currentTimeMillis()
      outputTokens = 0L
    }
    activeTaskIds.add(taskId)

    if (!supportsLiveUpdates()) {
      startForegroundCompat(buildKeepAliveNotification())
      return
    }

    startForegroundCompat(buildLiveUpdateGeneratingNotification())
  }

  private fun handleGenerationProgress(extras: Bundle, startId: Int) {
    if (activeTaskIds.isEmpty()) {
      stopSelf(startId)
      return
    }
    check(extras.containsKey(EXTRA_OUTPUT_TOKENS)) { "Missing output token count extra" }

    outputTokens = extras.getLong(EXTRA_OUTPUT_TOKENS)

    if (!supportsLiveUpdates()) {
      return
    }

    notificationManager.notify(NOTIFICATION_ID, buildLiveUpdateGeneratingNotification())
  }

  private fun handleGenerationFinish(extras: Bundle, startId: Int) {
    val taskId = requireTaskId(extras)
    check(extras.containsKey(EXTRA_OUTCOME)) { "Missing generation outcome extra" }
    check(extras.containsKey(EXTRA_STATUS_CODE)) { "Missing status code extra" }
    check(extras.containsKey(EXTRA_SHOW_COMPLETION_NOTIFICATION)) {
      "Missing show completion notification extra"
    }

    val outcome = requireNotNull(extras.getString(EXTRA_OUTCOME))
    require(outcome == OUTCOME_SUCCEEDED || outcome == OUTCOME_FAILED || outcome == OUTCOME_CANCELLED) {
      "Invalid generation outcome: $outcome"
    }
    val statusCode = extras.getInt(EXTRA_STATUS_CODE)
    val showCompletionNotification = extras.getBoolean(EXTRA_SHOW_COMPLETION_NOTIFICATION)

    if (!activeTaskIds.remove(taskId)) {
      Log.w(LOG_TAG, "Ignoring completion for unknown AI generation task: $taskId")
    }
    if (activeTaskIds.isNotEmpty()) {
      return
    }

    if (showCompletionNotification && outcome != OUTCOME_CANCELLED) {
      notifyCompletionNotification(outcome == OUTCOME_SUCCEEDED, statusCode)
    }

    startedAtMs = 0L
    outputTokens = 0L
    stopForegroundAndSelf(startId)
  }

  private fun requireTaskId(extras: Bundle): String {
    val taskId = requireNotNull(extras.getString(EXTRA_TASK_ID)).trim()
    require(taskId.isNotEmpty()) { "Missing generation task id extra" }
    return taskId
  }

  private fun stopForegroundAndSelf(startId: Int) {
    stopForeground(STOP_FOREGROUND_REMOVE)
    stopSelfResult(startId)
  }

  private fun notifyCompletionNotification(success: Boolean, statusCode: Int) {
    if (AndroidAppPresence.isForegroundInteractive()) {
      return
    }

    notificationManager.cancel(COMPLETION_NOTIFICATION_ID)
    notificationManager.notify(
      COMPLETION_NOTIFICATION_ID,
      if (success) {
        buildCompletionSuccessNotification()
      } else {
        buildCompletionFailureNotification(statusCode)
      },
    )
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

  private fun buildCompletionSuccessNotification(): Notification {
    val builder = NotificationCompat.Builder(this, AndroidAiGenerationNotifier.LIVE_UPDATE_CHANNEL_ID)
      .setSmallIcon(R.mipmap.ic_launcher)
      .setContentTitle(getString(R.string.notification_ai_done_title))
      .setContentText(getString(R.string.notification_ai_done_body))
      .setCategory(NotificationCompat.CATEGORY_STATUS)
      .setPriority(NotificationCompat.PRIORITY_DEFAULT)
      .setAutoCancel(true)
      .setContentIntent(AndroidAiGenerationNotifier.buildLaunchIntent(this))

    if (supportsLiveUpdates()) {
      builder
        .setShortCriticalText(getString(R.string.notification_ai_done_short))
        .setStyle(buildCompletedProgressStyle())
    }

    return builder
      .build()
  }

  private fun buildCompletionFailureNotification(statusCode: Int): Notification {
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
      .setAutoCancel(true)
      .setContentIntent(AndroidAiGenerationNotifier.buildLaunchIntent(this))
      .build()
  }

  private fun buildCompletedProgressStyle(): ProgressStyle {
    val pointColor = 0xFFECB7FF.toInt()
    val segmentColor = 0xFF86F7FA.toInt()

    return ProgressStyle()
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
  }

  companion object {
    private const val LOG_TAG = "TauriTavernAI"
    const val ACTION_GENERATION_START = "com.tauritavern.client.action.AI_GENERATION_START"
    const val ACTION_GENERATION_PROGRESS = "com.tauritavern.client.action.AI_GENERATION_PROGRESS"
    const val ACTION_GENERATION_FINISH = "com.tauritavern.client.action.AI_GENERATION_FINISH"

    const val EXTRA_TASK_ID = "com.tauritavern.client.extra.TASK_ID"
    const val EXTRA_OUTPUT_TOKENS = "com.tauritavern.client.extra.OUTPUT_TOKENS"
    const val EXTRA_OUTCOME = "com.tauritavern.client.extra.OUTCOME"
    const val EXTRA_STATUS_CODE = "com.tauritavern.client.extra.STATUS_CODE"
    const val EXTRA_SHOW_COMPLETION_NOTIFICATION =
      "com.tauritavern.client.extra.SHOW_COMPLETION_NOTIFICATION"

    const val OUTCOME_SUCCEEDED = "succeeded"
    const val OUTCOME_FAILED = "failed"
    const val OUTCOME_CANCELLED = "cancelled"

    const val NOTIFICATION_ID = 42000
    const val COMPLETION_NOTIFICATION_ID = 42001
  }
}
