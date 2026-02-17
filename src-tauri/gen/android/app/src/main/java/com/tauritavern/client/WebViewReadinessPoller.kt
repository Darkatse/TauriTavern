package com.tauritavern.client

import android.webkit.WebView

class WebViewReadinessPoller(
  private val webViewProvider: () -> WebView?,
  private val isDestroyed: () -> Boolean,
  private val maxAttempts: Int = DEFAULT_MAX_ATTEMPTS,
  private val retryDelayMs: Long = DEFAULT_RETRY_DELAY_MS,
) {
  fun pollUntilReady(
    readinessScript: String,
    onReady: () -> Unit,
    onFinished: () -> Unit,
    attempt: Int = 0,
  ) {
    val targetWebView = webViewProvider()
    if (targetWebView == null || isDestroyed()) {
      onFinished()
      return
    }

    targetWebView.post {
      val activeWebView = webViewProvider()
      if (activeWebView == null || isDestroyed()) {
        onFinished()
        return@post
      }

      activeWebView.evaluateJavascript(readinessScript) { value ->
        if (isDestroyed()) {
          onFinished()
          return@evaluateJavascript
        }

        if (value == "true") {
          onReady()
          onFinished()
          return@evaluateJavascript
        }

        if (attempt >= maxAttempts) {
          onFinished()
          return@evaluateJavascript
        }

        activeWebView.postDelayed(
          { pollUntilReady(readinessScript, onReady, onFinished, attempt + 1) },
          retryDelayMs,
        )
      }
    }
  }

  companion object {
    const val DEFAULT_MAX_ATTEMPTS = 100
    const val DEFAULT_RETRY_DELAY_MS = 80L
  }
}
