package com.tauritavern.client

import android.webkit.WebView
import androidx.annotation.MainThread
import androidx.activity.OnBackPressedCallback
import androidx.activity.OnBackPressedDispatcher
import androidx.lifecycle.LifecycleOwner

class AndroidBackNavigationController(
  private val webViewProvider: () -> WebView?,
  private val consumeNativeBack: () -> Boolean = { false },
  private val exitApp: () -> Unit,
) {
  fun register(dispatcher: OnBackPressedDispatcher, owner: LifecycleOwner) {
    dispatcher.addCallback(
      owner,
      object : OnBackPressedCallback(true) {
        override fun handleOnBackPressed() {
          handleBackPressed()
        }
      },
    )
  }

  @MainThread
  fun handleBackPressed() {
    if (consumeNativeBack()) {
      return
    }

    val activeWebView = webViewProvider()
    if (activeWebView == null) {
      exitApp()
      return
    }

    activeWebView.evaluateJavascript(HANDLE_BACK_SCRIPT) { value ->
      if (value == "true") {
        return@evaluateJavascript
      }

      exitApp()
    }
  }

  companion object {
    private val HANDLE_BACK_SCRIPT =
      """
      (() => {
        const fn = window.__TAURITAVERN_HANDLE_BACK__;
        return Boolean(typeof fn === 'function' && fn());
      })();
      """.trimIndent()
  }
}
