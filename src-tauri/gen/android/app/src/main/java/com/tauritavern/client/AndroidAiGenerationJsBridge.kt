package com.tauritavern.client

import android.os.Build
import android.os.Handler
import android.webkit.JavascriptInterface
import org.json.JSONObject

class AndroidAiGenerationJsBridge(
  private val mainHandler: Handler,
  private val notifier: AndroidAiGenerationNotifier,
) {
  @JavascriptInterface
  fun onGenerationStart() {
    mainHandler.post {
      notifier.onGenerationStart()
    }
  }

  @JavascriptInterface
  fun onGenerationProgress(outputTokens: Long) {
    mainHandler.post { notifier.onGenerationProgress(outputTokens) }
  }

  @JavascriptInterface
  fun onGenerationFinish(resultJson: String?) {
    val payload = JSONObject(requireNotNull(resultJson).trim())
    val success = payload.getBoolean("success")
    val statusCode = payload.getInt("status_code")
    val showCompletionNotification = payload.getBoolean("show_completion_notification")

    mainHandler.post {
      notifier.onGenerationFinish(
        success = success,
        statusCode = statusCode,
        showCompletionNotification = showCompletionNotification,
      )
    }
  }

  @JavascriptInterface
  fun onGenerationStop() {
    mainHandler.post { notifier.onGenerationStop() }
  }

  @JavascriptInterface
  fun supportsLiveUpdates(): Boolean {
    return Build.VERSION.SDK_INT >= Build.VERSION_CODES.BAKLAVA
  }

  companion object {
    const val INTERFACE_NAME = "TauriTavernAndroidAiBridge"
  }
}
