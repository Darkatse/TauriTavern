package com.tauritavern.client

import android.content.res.Configuration
import android.content.res.Resources
import android.graphics.Color
import android.os.Build
import android.os.Handler
import android.view.View
import android.view.Window
import android.view.WindowManager
import android.webkit.WebView
import androidx.core.graphics.Insets
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import java.util.Locale

class AndroidInsetsBridge(
  private val window: Window,
  private val resources: Resources,
  private val contentRootProvider: () -> View?,
  private val webViewProvider: () -> WebView?,
  private val isDestroyed: () -> Boolean,
  private val mainHandler: Handler,
  private val readinessPoller: WebViewReadinessPoller,
) {
  private var systemBarInsets: Insets = Insets.NONE
  private var imeBottomInset: Int = 0
  private var lastPushedInsetsSnapshot: InsetsSnapshot? = null
  private var isInsetsPushScheduled: Boolean = false
  private var hasPendingForcedInsetsPush: Boolean = false
  private var hasReadyPageInsetsInjection: Boolean = false
  private var isInsetsSyncScheduled: Boolean = false
  private var hasPendingInsetsSync: Boolean = false
  private var isInsetsListenerAttached: Boolean = false

  fun onCreate() {
    configureImmersiveSystemBars()
    attachSystemInsetsListenerIfNeeded()
    requestSystemInsets()
  }

  fun onConfigurationChanged() {
    configureImmersiveSystemBars()
    refreshInjection()
  }

  fun onWebViewAvailable() {
    lastPushedInsetsSnapshot = null
    hasReadyPageInsetsInjection = false
    refreshInjection()
  }

  fun onResume() {
    refreshInjection()
  }

  fun refreshInjection() {
    attachSystemInsetsListenerIfNeeded()
    requestSystemInsets()
    scheduleInsetsSyncWhenPageReady()
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

  private fun attachSystemInsetsListenerIfNeeded() {
    if (isInsetsListenerAttached) {
      return
    }

    val contentRoot = contentRootProvider() ?: return
    ViewCompat.setOnApplyWindowInsetsListener(contentRoot) { _, insets ->
      updateWindowInsets(insets)
      insets
    }
    isInsetsListenerAttached = true
  }

  private fun requestSystemInsets() {
    contentRootProvider()?.let { ViewCompat.requestApplyInsets(it) }
  }

  private fun updateWindowInsets(insets: WindowInsetsCompat) {
    updateSystemBarInsets(insets)
    updateImeInsets(insets)
    pushInsetsToWebView(force = false)
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
  }

  private fun updateImeInsets(insets: WindowInsetsCompat) {
    val imeType = WindowInsetsCompat.Type.ime()
    imeBottomInset = if (insets.isVisible(imeType)) insets.getInsets(imeType).bottom else 0
  }

  private fun pushInsetsToWebView(force: Boolean) {
    if (isDestroyed()) {
      return
    }

    val targetWebView = webViewProvider() ?: return
    hasPendingForcedInsetsPush = hasPendingForcedInsetsPush || force
    if (isInsetsPushScheduled) {
      return
    }
    isInsetsPushScheduled = true

    targetWebView.post {
      isInsetsPushScheduled = false
      val activeWebView = webViewProvider() ?: return@post
      val snapshot = InsetsSnapshot(systemBarInsets, imeBottomInset)
      val shouldForcePush = hasPendingForcedInsetsPush
      hasPendingForcedInsetsPush = false

      if (!shouldForcePush && snapshot == lastPushedInsetsSnapshot) {
        return@post
      }

      evaluateInsetsOnWebView(activeWebView, snapshot)
      lastPushedInsetsSnapshot = snapshot
      if (shouldForcePush) {
        hasReadyPageInsetsInjection = true
      }
    }
  }

  private fun evaluateInsetsOnWebView(targetWebView: WebView, snapshot: InsetsSnapshot) {
    val density = resources.displayMetrics.density

    fun toCssPx(value: Int): String = String.format(Locale.US, "%.2fpx", value / density)

    val script =
      """
      (() => {
        const root = document.documentElement;
        if (!root) return;
        root.style.setProperty('--tt-safe-area-top', '${toCssPx(snapshot.systemBars.top)}');
        root.style.setProperty('--tt-safe-area-right', '${toCssPx(snapshot.systemBars.right)}');
        root.style.setProperty('--tt-safe-area-left', '${toCssPx(snapshot.systemBars.left)}');
        root.style.setProperty('--tt-safe-area-bottom', '${toCssPx(snapshot.systemBars.bottom)}');
        root.style.setProperty('--tt-ime-bottom', '${toCssPx(snapshot.imeBottom)}');
      })();
      """.trimIndent()

    targetWebView.evaluateJavascript(script, null)
  }

  private fun scheduleInsetsSyncWhenPageReady() {
    if (webViewProvider() == null) {
      return
    }
    if (isInsetsSyncScheduled) {
      hasPendingInsetsSync = true
      return
    }

    isInsetsSyncScheduled = true
    hasPendingInsetsSync = false
    readinessPoller.pollUntilReady(
      readinessScript = PAGE_READY_SCRIPT,
      onReady = { pushInsetsToWebView(force = true) },
      onFinished = {
        isInsetsSyncScheduled = false
        if (hasPendingInsetsSync) {
          hasPendingInsetsSync = false
          scheduleInsetsSyncWhenPageReady()
        } else if (!hasReadyPageInsetsInjection) {
          mainHandler.postDelayed(
            { if (!isDestroyed()) scheduleInsetsSyncWhenPageReady() },
            WebViewReadinessPoller.DEFAULT_RETRY_DELAY_MS,
          )
        }
      },
    )
  }

  private data class InsetsSnapshot(val systemBars: Insets, val imeBottom: Int)

  companion object {
    private val PAGE_READY_SCRIPT =
      """
      (() => document.readyState !== 'loading' && location.href !== 'about:blank')();
      """.trimIndent()
  }
}
