package com.tauritavern.client

import android.os.Handler
import android.webkit.JavascriptInterface

class AndroidSystemUiJsBridge(
  private val mainHandler: Handler,
  private val windowLayout: AndroidWindowLayout,
) {
  @JavascriptInterface
  fun setImmersiveFullscreenEnabled(enabled: Boolean) {
    mainHandler.post { windowLayout.setImmersiveFullscreenEnabled(enabled) }
  }

  @JavascriptInterface
  fun isImmersiveFullscreenEnabled(): Boolean = windowLayout.isImmersiveFullscreenEnabled()

  companion object {
    const val INTERFACE_NAME = "TauriTavernAndroidSystemUiBridge"
  }
}
