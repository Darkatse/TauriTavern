package com.tauritavern.client

import android.os.Handler
import android.webkit.JavascriptInterface

class AndroidAiGenerationJsBridge(
  private val mainHandler: Handler,
  private val notifier: AndroidAiGenerationNotifier,
) {
  @JavascriptInterface
  fun onGenerationStart() {
    mainHandler.post { notifier.ensureKeepAliveService() }
  }

  @JavascriptInterface
  fun onGenerationStop() {
    // Keep this interface for JS compatibility. Keepalive is app-session scoped now.
  }

  companion object {
    const val INTERFACE_NAME = "TauriTavernAndroidAiBridge"
  }
}
