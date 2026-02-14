package com.tauritavern.app

import android.content.res.Configuration
import android.graphics.Color
import android.os.Build
import android.os.Bundle
import android.view.View
import android.view.WindowManager
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge
import androidx.core.graphics.Insets
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import java.util.Locale

class MainActivity : TauriActivity() {
  private var webView: WebView? = null
  private var systemBarInsets: Insets = Insets.NONE

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    configureImmersiveSystemBars()
    attachSystemInsetsListener()
    requestSystemInsets()
  }

  override fun onConfigurationChanged(newConfig: Configuration) {
    super.onConfigurationChanged(newConfig)
    configureImmersiveSystemBars()
    refreshInsetsInjection()
  }

  override fun onWebViewCreate(webView: WebView) {
    this.webView = webView
    refreshInsetsInjection()
  }

  override fun onResume() {
    super.onResume()
    refreshInsetsInjection()
  }

  @Suppress("DEPRECATION")
  private fun configureImmersiveSystemBars() {
    WindowCompat.setDecorFitsSystemWindows(window, false)
    window.statusBarColor = Color.TRANSPARENT
    window.navigationBarColor = Color.TRANSPARENT

    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
      window.attributes = window.attributes.apply {
        layoutInDisplayCutoutMode =
          WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES
      }
    }

    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
      window.isStatusBarContrastEnforced = false
      window.isNavigationBarContrastEnforced = false
    }

    val isDarkMode =
      (resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK) ==
        Configuration.UI_MODE_NIGHT_YES

    WindowInsetsControllerCompat(window, window.decorView).apply {
      isAppearanceLightStatusBars = !isDarkMode
      isAppearanceLightNavigationBars = !isDarkMode
    }
  }

  private fun attachSystemInsetsListener() {
    val contentRoot = getContentRoot() ?: return

    ViewCompat.setOnApplyWindowInsetsListener(contentRoot) { _, insets ->
      updateSystemBarInsets(insets)
      insets
    }
  }

  private fun updateSystemBarInsets(insets: WindowInsetsCompat) {
    val insetTypes = WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout()
    val visibleInsets = insets.getInsets(insetTypes)
    val stableInsets = insets.getInsetsIgnoringVisibility(insetTypes)
    systemBarInsets =
      Insets.of(
        maxOf(visibleInsets.left, stableInsets.left),
        maxOf(visibleInsets.top, stableInsets.top),
        maxOf(visibleInsets.right, stableInsets.right),
        maxOf(visibleInsets.bottom, stableInsets.bottom),
      )
    pushInsetsToWebView()
  }

  private fun requestSystemInsets() {
    getContentRoot()?.let { ViewCompat.requestApplyInsets(it) }
  }

  private fun getContentRoot(): View? = window.decorView.findViewById(android.R.id.content)

  private fun pushInsetsToWebView() {
    val targetWebView = webView ?: return
    val density = resources.displayMetrics.density
    fun toCssPx(value: Int): String =
      String.format(Locale.US, "%.2fpx", value / density)

    val script =
      """
      (() => {
        const root = document.documentElement;
        if (!root) return;
        root.style.setProperty('--tt-safe-area-top', '${toCssPx(systemBarInsets.top)}');
        root.style.setProperty('--tt-safe-area-right', '${toCssPx(systemBarInsets.right)}');
        root.style.setProperty('--tt-safe-area-left', '${toCssPx(systemBarInsets.left)}');
        root.style.setProperty('--tt-safe-area-bottom', '${toCssPx(systemBarInsets.bottom)}');
      })();
      """.trimIndent()

    targetWebView.post {
      targetWebView.evaluateJavascript(script, null)
    }
  }

  private fun syncInsetsWhenPageReady(attempt: Int = 0) {
    val targetWebView = webView ?: return
    val maxAttempts = 100
    val retryDelayMs = 80L

    val readinessScript =
      """
      (() => document.readyState !== 'loading' && location.href !== 'about:blank')();
      """.trimIndent()

    targetWebView.post {
      targetWebView.evaluateJavascript(
        readinessScript,
      ) { value ->
        if (value == "true") {
          pushInsetsToWebView()
          return@evaluateJavascript
        }
        if (attempt < maxAttempts) {
          targetWebView.postDelayed({ syncInsetsWhenPageReady(attempt + 1) }, retryDelayMs)
        }
      }
    }
  }

  private fun refreshInsetsInjection() {
    requestSystemInsets()
    syncInsetsWhenPageReady()
  }
}
