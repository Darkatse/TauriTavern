package com.tauritavern.client

import android.app.Notification
import android.app.NotificationManager
import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.Bundle
import android.os.IBinder
import android.os.PowerManager
import android.util.Log
import androidx.annotation.StringRes
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationCompat.ProgressStyle
import androidx.core.graphics.drawable.IconCompat

data class KeepAliveState(
  val active: Boolean,
  val needsCpu: Boolean,
  val persistent: Boolean,
)

class AiGenerationForegroundService : Service() {
  private val notificationManager: NotificationManager by lazy {
    getSystemService(NOTIFICATION_SERVICE) as NotificationManager
  }

  private val wakeLock = CpuWakeLock()
  private var currentState = KeepAliveState(active = false, needsCpu = false, persistent = false)
  private var startedAtMs: Long = 0L
  private var outputTokens: Long = 0L

  private inner class CpuWakeLock {
    private var lock: PowerManager.WakeLock? = null

    fun acquire(timeoutMs: Long) {
      val current =
        lock
          ?: (getSystemService(POWER_SERVICE) as PowerManager)
            .newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, WAKE_LOCK_TAG)
            .apply { setReferenceCounted(false) }
            .also { lock = it }
      if (!current.isHeld) {
        current.acquire(timeoutMs)
      }
    }

    fun releaseIfHeld() {
      val current = lock ?: return
      if (current.isHeld) {
        current.release()
      }
    }
  }

  override fun onBind(intent: Intent?): IBinder? = null

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    AndroidAiGenerationNotifier.ensureNotificationChannels(this)

    when (intent?.action) {
      null -> stopSelf(startId)
      ACTION_SYNC_STATE -> handleSyncState(requireNotNull(intent.extras), startId)
      ACTION_GENERATION_PROGRESS ->
        handleGenerationProgress(requireNotNull(intent.extras), startId)
      else -> error("Unknown intent action: ${intent.action}")
    }

    return START_NOT_STICKY
  }

  override fun onDestroy() {
    currentState = KeepAliveState(active = false, needsCpu = false, persistent = false)
    startedAtMs = 0L
    outputTokens = 0L
    wakeLock.releaseIfHeld()
    stopForeground(STOP_FOREGROUND_REMOVE)
    super.onDestroy()
  }

  override fun onTimeout(startId: Int, fgsType: Int) {
    Log.w(LOG_TAG, "AI generation foreground service reached the Android time limit")
    startedAtMs = 0L
    outputTokens = 0L
    render(KeepAliveState(active = false, needsCpu = false, persistent = false), startId)
  }

  private fun supportsLiveUpdates(): Boolean {
    return Build.VERSION.SDK_INT >= Build.VERSION_CODES.BAKLAVA
  }

  private fun handleSyncState(extras: Bundle, startId: Int) {
    check(extras.containsKey(EXTRA_ACTIVE)) { "Missing active state extra" }
    check(extras.containsKey(EXTRA_NEEDS_CPU)) { "Missing needsCpu state extra" }
    check(extras.containsKey(EXTRA_PERSISTENT)) { "Missing persistent state extra" }

    val state =
      KeepAliveState(
        active = extras.getBoolean(EXTRA_ACTIVE),
        needsCpu = extras.getBoolean(EXTRA_NEEDS_CPU),
        persistent = extras.getBoolean(EXTRA_PERSISTENT),
      )

    if (extras.containsKey(EXTRA_OUTCOME)) {
      check(extras.containsKey(EXTRA_STATUS_CODE)) { "Missing status code extra" }
      check(extras.containsKey(EXTRA_SHOW_COMPLETION_NOTIFICATION)) {
        "Missing show completion notification extra"
      }

      val outcome = requireNotNull(extras.getString(EXTRA_OUTCOME))
      require(
        outcome == OUTCOME_SUCCEEDED || outcome == OUTCOME_FAILED || outcome == OUTCOME_CANCELLED,
      ) {
        "Invalid generation outcome: $outcome"
      }
      val statusCode = extras.getInt(EXTRA_STATUS_CODE)
      val showCompletionNotification = extras.getBoolean(EXTRA_SHOW_COMPLETION_NOTIFICATION)

      if (showCompletionNotification && outcome != OUTCOME_CANCELLED) {
        notifyCompletionNotification(outcome == OUTCOME_SUCCEEDED, statusCode)
      }
    }

    if (state.needsCpu && !currentState.needsCpu) {
      startedAtMs = System.currentTimeMillis()
      outputTokens = 0L
    } else if (!state.needsCpu) {
      startedAtMs = 0L
      outputTokens = 0L
    }

    render(state, startId)
  }

  private fun render(state: KeepAliveState, @Suppress("UNUSED_PARAMETER") startId: Int) {
    val wasNeedsCpu = currentState.needsCpu
    currentState = state
    if (!state.active) {
      wakeLock.releaseIfHeld()
      stopForeground(STOP_FOREGROUND_REMOVE)
      stopSelf()
      return
    }
    if (state.needsCpu) wakeLock.acquire(WAKE_LOCK_TIMEOUT_MS) else wakeLock.releaseIfHeld()
    if (wasNeedsCpu && !state.needsCpu && supportsLiveUpdates()) {
      stopForeground(STOP_FOREGROUND_REMOVE)
    }
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
      startForeground(NOTIFICATION_ID, notificationFor(state), foregroundTypeFor(state))
    } else {
      startForeground(NOTIFICATION_ID, notificationFor(state))
    }
  }

  private fun notificationFor(state: KeepAliveState): Notification {
    if (!state.needsCpu) {
      return buildKeepAliveNotification(
        titleRes = R.string.notification_keepalive_title,
        bodyRes = R.string.notification_keepalive_body,
      )
    }
    if (!supportsLiveUpdates()) {
      return buildKeepAliveNotification(
        titleRes = R.string.notification_ai_keepalive_title,
        bodyRes = R.string.notification_ai_keepalive_body,
      )
    }
    return buildLiveUpdateGeneratingNotification()
  }

  private fun foregroundTypeFor(state: KeepAliveState): Int {
    if (state.persistent && Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
      return ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE
    }
    return ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC
  }

  private fun handleGenerationProgress(extras: Bundle, startId: Int) {
    if (!currentState.needsCpu) {
      if (!currentState.active) {
        stopSelf(startId)
      }
      return
    }
    check(extras.containsKey(EXTRA_OUTPUT_TOKENS)) { "Missing output token count extra" }

    outputTokens = extras.getLong(EXTRA_OUTPUT_TOKENS)

    if (!supportsLiveUpdates()) {
      return
    }

    notificationManager.notify(NOTIFICATION_ID, buildLiveUpdateGeneratingNotification())
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

  private fun buildKeepAliveNotification(
    @StringRes titleRes: Int,
    @StringRes bodyRes: Int,
  ): Notification {
    return NotificationCompat.Builder(this, AndroidAiGenerationNotifier.KEEPALIVE_CHANNEL_ID)
      .setSmallIcon(R.mipmap.ic_launcher)
      .setContentTitle(getString(titleRes))
      .setContentText(getString(bodyRes))
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
    private const val WAKE_LOCK_TAG = "TauriTavern:AiGenerationWakeLock"
    private const val WAKE_LOCK_TIMEOUT_MS = 30 * 60 * 1000L

    const val ACTION_SYNC_STATE = "com.tauritavern.client.action.AI_GENERATION_SYNC_STATE"
    const val ACTION_GENERATION_PROGRESS = "com.tauritavern.client.action.AI_GENERATION_PROGRESS"

    const val EXTRA_ACTIVE = "com.tauritavern.client.extra.ACTIVE"
    const val EXTRA_NEEDS_CPU = "com.tauritavern.client.extra.NEEDS_CPU"
    const val EXTRA_PERSISTENT = "com.tauritavern.client.extra.PERSISTENT"
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
