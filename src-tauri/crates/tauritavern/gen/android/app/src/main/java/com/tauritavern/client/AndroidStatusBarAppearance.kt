package com.tauritavern.client

import android.graphics.Bitmap
import android.graphics.Rect
import android.os.Build
import android.os.Handler
import android.os.SystemClock
import android.view.PixelCopy
import android.view.ViewTreeObserver
import android.view.Window
import androidx.core.graphics.ColorUtils
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat

/** Chooses status bar icons from the rendered background, without changing its transparency. */
class AndroidStatusBarAppearance(
  private val window: Window,
  private val mainHandler: Handler,
) {
  var useDarkIcons: Boolean? = null
    private set

  private var running = false
  private var needsSample = false
  private var sampleScheduled = false
  private var copyInFlight = false
  private var lastSampleTime = 0L
  private val bitmap by lazy { Bitmap.createBitmap(32, 4, Bitmap.Config.ARGB_8888) }
  private val sampleTask = Runnable { sampleBackground() }
  private val frameRendered = Runnable {
    if (running && isStatusBarVisible()) {
      needsSample = true
      scheduleSample()
    }
  }
  private val frameCommitted = Runnable { mainHandler.post(frameRendered) }
  private val drawListener = ViewTreeObserver.OnPreDrawListener {
    if (running && isStatusBarVisible()) {
      val view = window.decorView
      // Register before drawing: Android collects commit callbacks before OnDraw.
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q && view.isHardwareAccelerated) {
        view.viewTreeObserver.registerFrameCommitCallback(frameCommitted)
      } else {
        // Android 8/9 have no commit callback; defer until after the drawing pass.
        mainHandler.post(frameRendered)
      }
    }
    true
  }

  fun start() {
    if (running) return
    running = true
    window.decorView.viewTreeObserver.addOnPreDrawListener(drawListener)
  }

  fun stop() {
    if (!running) return
    running = false
    val observer = window.decorView.viewTreeObserver
    if (observer.isAlive) observer.removeOnPreDrawListener(drawListener)
    mainHandler.removeCallbacks(frameRendered)
    mainHandler.removeCallbacks(sampleTask)
    sampleScheduled = false
    needsSample = false
  }

  private fun isStatusBarVisible(): Boolean =
    ViewCompat.getRootWindowInsets(window.decorView)
      ?.isVisible(WindowInsetsCompat.Type.statusBars()) == true

  private fun scheduleSample() {
    if (!running || !needsSample || sampleScheduled || copyInFlight) return
    sampleScheduled = true
    mainHandler.postAtTime(
      sampleTask,
      maxOf(SystemClock.uptimeMillis(), lastSampleTime + SAMPLE_INTERVAL_MS),
    )
  }

  private fun sampleBackground() {
    sampleScheduled = false
    needsSample = false
    if (!running || !isStatusBarVisible()) return

    val view = window.decorView
    val height = ViewCompat.getRootWindowInsets(view)
      ?.getInsets(WindowInsetsCompat.Type.statusBars())?.top ?: 0
    if (view.width == 0 || height == 0) return

    copyInFlight = true
    lastSampleTime = SystemClock.uptimeMillis()
    try {
      PixelCopy.request(
        window,
        Rect(0, 0, view.width, height),
        bitmap,
        { result ->
          copyInFlight = false
          if (running && isStatusBarVisible()) {
            if (result == PixelCopy.SUCCESS) {
              updateIcons()
            } else {
              Logger.warn("StatusBarAppearance", "Background sampling failed: PixelCopy result $result")
            }
          }
          // Preserve a draw that arrived during the copy, including the final animation frame.
          scheduleSample()
        },
        mainHandler,
      )
    } catch (error: IllegalArgumentException) {
      copyInFlight = false
      Logger.warn("StatusBarAppearance", "Background sampling unavailable: $error")
    }
  }

  private fun updateIcons() {
    var luminance = 0.0
    for (y in 0 until bitmap.height) {
      for (x in 0 until bitmap.width) {
        luminance += ColorUtils.calculateLuminance(bitmap.getPixel(x, y))
      }
    }
    luminance /= bitmap.width * bitmap.height
    // Black and white have equal contrast at relative luminance sqrt(0.05 * 1.05) - 0.05.
    val darkIcons = luminance > 0.179
    if (useDarkIcons == darkIcons) return
    WindowCompat.getInsetsController(window, window.decorView)
      .isAppearanceLightStatusBars = darkIcons
    useDarkIcons = darkIcons
  }

  companion object {
    private const val SAMPLE_INTERVAL_MS = 1_000L
  }
}
