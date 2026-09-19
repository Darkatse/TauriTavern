package com.tauritavern.client

import android.app.Activity
import android.os.Handler
import android.os.Looper
import android.util.Log
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.Plugin

@InvokeArg
class StartGenerationArgs {
  lateinit var taskId: String
}

@InvokeArg
class FinishGenerationArgs {
  lateinit var taskId: String
  lateinit var outcome: String
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
  fun start(invoke: Invoke) {
    try {
      val args = invoke.parseArgs(StartGenerationArgs::class.java)
      mainHandler.post {
        try {
          notifier.onGenerationStart(args.taskId)
        } catch (error: Exception) {
          Log.w(LOG_TAG, "Failed to start AI generation foreground service", error)
        }
      }
      invoke.resolve()
    } catch (error: Exception) {
      invoke.reject(error.message)
    }
  }

  @Command
  fun finish(invoke: Invoke) {
    try {
      val args = invoke.parseArgs(FinishGenerationArgs::class.java)
      mainHandler.post {
        try {
          notifier.onGenerationFinish(
            taskId = args.taskId,
            outcome = args.outcome,
            statusCode = args.statusCode,
            notifyCompletion = args.notifyCompletion,
          )
        } catch (error: Exception) {
          Log.w(LOG_TAG, "Failed to finish AI generation foreground service", error)
        }
      }
      invoke.resolve()
    } catch (error: Exception) {
      invoke.reject(error.message)
    }
  }

  companion object {
    private const val LOG_TAG = "TauriTavernAI"
  }
}
