package com.tauritavern.client

import android.os.Build
import android.os.Handler
import android.webkit.JavascriptInterface

class AndroidAiGenerationJsBridge(
  private val mainHandler: Handler,
  private val notifier: AndroidAiGenerationNotifier,
) {
  @JavascriptInterface
  fun onGenerationProgress(outputTokens: Long) {
    mainHandler.post { notifier.onGenerationProgress(outputTokens) }
  }

  @JavascriptInterface
  fun supportsLiveUpdates(): Boolean {
    return Build.VERSION.SDK_INT >= Build.VERSION_CODES.BAKLAVA
  }

  @JavascriptInterface
  fun supportsNativeCompletion(): Boolean {
    return true
  }

  companion object {
    const val INTERFACE_NAME = "TauriTavernAndroidAiBridge"
  }
}
