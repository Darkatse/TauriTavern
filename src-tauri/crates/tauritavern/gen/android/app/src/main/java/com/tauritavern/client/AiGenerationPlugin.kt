package com.tauritavern.client

import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.Intent
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.provider.Settings
import android.util.Log
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.Plugin

@InvokeArg
class SyncKeepAliveStateArgs {
  var active: Boolean = false
  var needsCpu: Boolean = false
  var persistent: Boolean = false
  var outcome: String? = null
  var statusCode: Int = 0
  var notifyCompletion: Boolean = false
}

@TauriPlugin
class AiGenerationPlugin(
  private val activity: Activity,
) : Plugin(activity) {
  private val notifier = AndroidAiGenerationNotifier(activity.applicationContext)
  private val mainHandler = Handler(Looper.getMainLooper())

  @Command
  fun syncState(invoke: Invoke) {
    try {
      val args = invoke.parseArgs(SyncKeepAliveStateArgs::class.java)
      mainHandler.post {
        try {
          notifier.syncState(
            state =
              KeepAliveState(
                active = args.active,
                needsCpu = args.needsCpu,
                persistent = args.persistent,
              ),
            outcome = args.outcome,
            statusCode = args.statusCode,
            notifyCompletion = args.notifyCompletion,
          )
        } catch (error: Exception) {
          Log.w(LOG_TAG, "Failed to sync AI generation foreground service state", error)
        }
      }
      invoke.resolve()
    } catch (error: Exception) {
      invoke.reject(error.message)
    }
  }

  @Command
  fun requestIgnoreBatteryOptimizations(invoke: Invoke) {
    val intent =
      Intent(
        Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS,
        Uri.parse("package:${activity.packageName}"),
      )
    try {
      activity.startActivity(intent)
      invoke.resolve()
    } catch (error: ActivityNotFoundException) {
      invoke.reject(error.message)
    }
  }

  companion object {
    private const val LOG_TAG = "TauriTavernAI"
  }
}
