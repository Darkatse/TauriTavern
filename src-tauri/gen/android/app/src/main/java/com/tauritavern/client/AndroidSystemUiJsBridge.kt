package com.tauritavern.client

import android.os.Handler
import android.webkit.JavascriptInterface

class AndroidSystemUiJsBridge(
  private val mainHandler: Handler,
  private val insetsBridge: AndroidInsetsBridge,
) {
  @JavascriptInterface
  fun setImmersiveFullscreenEnabled(enabled: Boolean) {
    mainHandler.post { insetsBridge.setImmersiveFullscreenEnabled(enabled) }
  }

  @JavascriptInterface
  fun isImmersiveFullscreenEnabled(): Boolean = insetsBridge.isImmersiveFullscreenEnabled()

  companion object {
    const val INTERFACE_NAME = "TauriTavernAndroidSystemUiBridge"
  }
}
