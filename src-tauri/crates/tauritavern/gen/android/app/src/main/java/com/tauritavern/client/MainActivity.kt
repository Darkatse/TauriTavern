package com.tauritavern.client

import android.app.AlertDialog
import android.content.Intent
import android.content.res.Configuration
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.util.Log
import android.view.View
import android.view.ViewGroup
import android.webkit.WebView
import android.webkit.WebChromeClient
import android.widget.Toast
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.RejectedExecutionException

class MainActivity : TauriActivity(), AndroidWebFullscreenHost {
  private var webView: WebView? = null
  private val mainHandler = Handler(Looper.getMainLooper())
  private val backgroundExecutor: ExecutorService =
    Executors.newSingleThreadExecutor { runnable ->
      Thread(runnable, "tauritavern-main-bg").apply { priority = Thread.NORM_PRIORITY - 1 }
    }
  private var isActivityDestroyed: Boolean = false
  private var fontResetPending = false
  private var fontResetInProgress = false
  private val backNavigationController: AndroidBackNavigationController by lazy {
    AndroidBackNavigationController(
      webViewProvider = { webView },
      consumeNativeBack = { webFullscreenController.hide() },
      exitApp = { finish() },
    )
  }
  private val aiGenerationNotifier: AndroidAiGenerationNotifier by lazy {
    AndroidAiGenerationNotifier(applicationContext)
  }
  private val aiGenerationJsBridge: AndroidAiGenerationJsBridge by lazy {
    AndroidAiGenerationJsBridge(mainHandler, aiGenerationNotifier)
  }
  private val systemUiJsBridge: AndroidSystemUiJsBridge by lazy {
    AndroidSystemUiJsBridge(mainHandler, windowLayout)
  }
  private val readinessPoller: WebViewReadinessPoller by lazy {
    WebViewReadinessPoller(webViewProvider = { webView }, isDestroyed = { isActivityDestroyed })
  }

  val windowLayout: AndroidWindowLayout by lazy {
    AndroidWindowLayout(
      window = window,
      resources = resources,
      contentRootProvider = { window.decorView.findViewById(android.R.id.content) },
      webViewProvider = { webView },
    )
  }
  private val webFullscreenController: AndroidWebFullscreenController by lazy {
    AndroidWebFullscreenController(
      contentRootProvider = { window.decorView.findViewById<ViewGroup>(android.R.id.content) },
      windowLayout = windowLayout,
    )
  }

  private val shareIntentParser: ShareIntentParser by lazy {
    ShareIntentParser(contentResolver = contentResolver)
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
    super.onCreate(savedInstanceState)
    backNavigationController.register(onBackPressedDispatcher, this)
    aiGenerationNotifier.acknowledgeCompletionNotification()
    windowLayout.onCreate()
    fontResetPending = savedInstanceState?.getBoolean(FONT_RESET_PENDING) == true
    handleLaunchIntent(intent)
    logWebViewTypography("onCreate")
  }

  override fun onNewIntent(intent: Intent) {
    super.onNewIntent(intent)
    setIntent(intent)
    aiGenerationNotifier.acknowledgeCompletionNotification()
    handleLaunchIntent(intent)
  }

  override fun onConfigurationChanged(newConfig: Configuration) {
    super.onConfigurationChanged(newConfig)
    applyWebViewTypographyBaseline("onConfigurationChanged")
    windowLayout.onConfigurationChanged()
  }

  override fun onWebViewCreate(webView: WebView) {
    this.webView = webView
    applyWebViewTypographyBaseline("onWebViewCreate")
    webView.addJavascriptInterface(aiGenerationJsBridge, AndroidAiGenerationJsBridge.INTERFACE_NAME)
    webView.addJavascriptInterface(systemUiJsBridge, AndroidSystemUiJsBridge.INTERFACE_NAME)
    windowLayout.onWebViewAvailable()
    if (BuildConfig.DEBUG) {
      webView.addOnLayoutChangeListener { _, l, t, r, b, oldL, oldT, oldR, oldB ->
        if (l != oldL || t != oldT || r != oldR || b != oldB) {
          logWebViewTypography("layout")
        }
      }
    }
    dispatchFontReset()
    sharePayloadDispatcher.requestDispatch()
  }

  override fun onResume() {
    super.onResume()
    applyWebViewTypographyBaseline("onResume")
    acknowledgeCompletionNotificationIfForeground(
      AndroidAppPresence.setActivityResumed(true),
    )
    windowLayout.onResume()
    dispatchFontReset()
    sharePayloadDispatcher.requestDispatch()
  }

  override fun onPause() {
    AndroidAppPresence.setActivityResumed(false)
    super.onPause()
  }

  override fun onSaveInstanceState(outState: Bundle) {
    super.onSaveInstanceState(outState)
    outState.putBoolean(FONT_RESET_PENDING, fontResetPending)
  }

  override fun onWindowFocusChanged(hasFocus: Boolean) {
    super.onWindowFocusChanged(hasFocus)
    acknowledgeCompletionNotificationIfForeground(
      AndroidAppPresence.setWindowFocused(hasFocus),
    )
  }

  override fun showWebFullscreenView(
    view: View,
    callback: WebChromeClient.CustomViewCallback,
  ): Boolean = webFullscreenController.show(view, callback)

  override fun hideWebFullscreenView(): Boolean = webFullscreenController.hide()

  override fun onDestroy() {
    isActivityDestroyed = true
    mainHandler.removeCallbacksAndMessages(null)
    backgroundExecutor.shutdownNow()
    super.onDestroy()
  }

  private fun acknowledgeCompletionNotificationIfForeground(enteredForegroundInteractive: Boolean) {
    if (enteredForegroundInteractive) {
      aiGenerationNotifier.acknowledgeCompletionNotification()
    }
  }

  private fun applyWebViewTypographyBaseline(event: String) {
    val settings = webView?.settings
    logWebViewTypography(event)
    if (settings != null && settings.textZoom != 100) {
      settings.textZoom = 100
      logWebViewTypography("textZoom applied")
    }
  }

  private fun logWebViewTypography(event: String) {
    if (!BuildConfig.DEBUG) return
    val view = webView
    Log.d(
      "TauriTavernDisplay",
      "activity=${System.identityHashCode(this)} event=$event " +
        "fontScale=${resources.configuration.fontScale} densityDpi=${resources.displayMetrics.densityDpi} " +
        "textZoom=${view?.settings?.textZoom} size=${view?.width}x${view?.height} " +
        "bounds=${view?.left},${view?.top},${view?.right},${view?.bottom}",
    )
  }

  private fun handleLaunchIntent(intent: Intent?) {
    if (intent?.action != FontResetShortcutActivity.RESET_FONT_ACTION) {
      captureShareIntent(intent)
      return
    }
    // Consume the action before configuration changes can recreate the Activity.
    setIntent(Intent(intent).setAction(Intent.ACTION_MAIN))
    if (fontResetPending) return
    AlertDialog.Builder(this)
      .setTitle(R.string.reset_display_fonts)
      .setMessage(R.string.reset_display_fonts_confirmation)
      .setNegativeButton(android.R.string.cancel, null)
      .setPositiveButton(android.R.string.ok) { _, _ ->
        fontResetPending = true
        showFontResetStatus(R.string.reset_display_fonts_pending)
        dispatchFontReset()
      }
      .show()
  }

  private fun dispatchFontReset() {
    val view = webView ?: return
    if (isActivityDestroyed || !fontResetPending || fontResetInProgress) return
    fontResetInProgress = true
    var completed = false
    readinessPoller.pollUntilReady(
      readinessScript = """
        (() => {
          const display = window.__TAURITAVERN__?.api?.display;
          if (!display) return false;
          const result = display.getFontResetResult();
          if (result === null) void display.resetFontScales();
          return result === 'saved' || result === 'failed';
        })()
      """.trimIndent(),
      onReady = {
        completed = true
        view.evaluateJavascript("window.__TAURITAVERN__?.api?.display?.takeFontResetResult()") { result ->
          if (isActivityDestroyed) return@evaluateJavascript
          fontResetInProgress = false
          when (result) {
            "\"saved\"", "\"failed\"" -> {
              fontResetPending = false
              showFontResetStatus(
                if (result == "\"saved\"") R.string.reset_display_fonts_saved else R.string.reset_display_fonts_failed,
              )
            }
            else -> dispatchFontReset()
          }
        }
      },
      onFinished = {
        if (!completed && !isActivityDestroyed) {
          mainHandler.postDelayed({
            fontResetInProgress = false
            dispatchFontReset()
          }, WebViewReadinessPoller.DEFAULT_RETRY_DELAY_MS)
        }
      },
    )
  }

  private fun showFontResetStatus(message: Int) {
    Toast.makeText(this, message, Toast.LENGTH_LONG).show()
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

  companion object {
    private const val FONT_RESET_PENDING = "fontResetPending"
  }
}
