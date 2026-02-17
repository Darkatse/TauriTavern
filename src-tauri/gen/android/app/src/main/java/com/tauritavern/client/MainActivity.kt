package com.tauritavern.client

import android.content.Intent
import android.content.res.Configuration
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.RejectedExecutionException

class MainActivity : TauriActivity() {
  private var webView: WebView? = null
  private val mainHandler = Handler(Looper.getMainLooper())
  private val backgroundExecutor: ExecutorService =
    Executors.newSingleThreadExecutor { runnable ->
      Thread(runnable, "tauritavern-main-bg").apply { priority = Thread.NORM_PRIORITY - 1 }
    }
  private var isActivityDestroyed: Boolean = false

  private val readinessPoller: WebViewReadinessPoller by lazy {
    WebViewReadinessPoller(webViewProvider = { webView }, isDestroyed = { isActivityDestroyed })
  }

  private val insetsBridge: AndroidInsetsBridge by lazy {
    AndroidInsetsBridge(
      window = window,
      resources = resources,
      contentRootProvider = { window.decorView.findViewById(android.R.id.content) },
      webViewProvider = { webView },
      isDestroyed = { isActivityDestroyed },
      mainHandler = mainHandler,
      readinessPoller = readinessPoller,
    )
  }

  private val shareIntentParser: ShareIntentParser by lazy {
    ShareIntentParser(contentResolver = contentResolver, cacheDir = cacheDir)
  }

  private val sharePayloadDispatcher: SharePayloadDispatcher by lazy {
    SharePayloadDispatcher(
      webViewProvider = { webView },
      isDestroyed = { isActivityDestroyed },
      mainHandler = mainHandler,
      readinessPoller = readinessPoller,
    )
  }

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    insetsBridge.onCreate()
    captureShareIntent(intent)
  }

  override fun onNewIntent(intent: Intent) {
    super.onNewIntent(intent)
    setIntent(intent)
    captureShareIntent(intent)
  }

  override fun onConfigurationChanged(newConfig: Configuration) {
    super.onConfigurationChanged(newConfig)
    insetsBridge.onConfigurationChanged()
  }

  override fun onWebViewCreate(webView: WebView) {
    this.webView = webView
    insetsBridge.onWebViewAvailable()
    sharePayloadDispatcher.requestDispatch()
  }

  override fun onResume() {
    super.onResume()
    insetsBridge.onResume()
    sharePayloadDispatcher.requestDispatch()
  }

  override fun onDestroy() {
    isActivityDestroyed = true
    mainHandler.removeCallbacksAndMessages(null)
    backgroundExecutor.shutdownNow()
    super.onDestroy()
  }

  private fun captureShareIntent(intent: Intent?) {
    val incomingIntent = intent ?: return
    if (!shareIntentParser.canHandle(incomingIntent)) {
      return
    }

    runOnBackground {
      val payloads = shareIntentParser.parse(Intent(incomingIntent))
      if (payloads.isEmpty()) {
        return@runOnBackground
      }

      mainHandler.post {
        if (isActivityDestroyed) {
          return@post
        }
        sharePayloadDispatcher.enqueue(payloads)
      }
    }
  }

  private fun runOnBackground(task: () -> Unit) {
    try {
      backgroundExecutor.execute(task)
    } catch (_: RejectedExecutionException) {
      // Activity is shutting down.
    }
  }
}
