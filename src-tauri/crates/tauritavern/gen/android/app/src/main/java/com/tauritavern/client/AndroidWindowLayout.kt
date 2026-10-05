package com.tauritavern.client

import android.content.res.Configuration
import android.content.res.Resources
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.ColorFilter
import android.graphics.Paint
import android.graphics.PixelFormat
import android.graphics.drawable.Drawable
import android.os.Handler
import android.view.View
import android.view.ViewGroup
import android.view.Window
import android.webkit.WebView
import androidx.core.graphics.Insets
import androidx.core.view.OneShotPreDrawListener
import androidx.core.view.ViewCompat
import androidx.core.view.ViewGroupCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsAnimationCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import org.json.JSONObject

/** Owns the browser's content rectangle. IME geometry never crosses into JavaScript. */
class AndroidWindowLayout(
  private val window: Window,
  private val resources: Resources,
  private val contentRootProvider: () -> ViewGroup?,
  private val webViewProvider: () -> WebView?,
  mainHandler: Handler,
) {
  private var immersiveFullscreenEnabled = true
  private var elementFullscreen = false
  private var attachedRoot: ViewGroup? = null
  private var targetInsets: WindowInsetsCompat? = null
  private val animations = mutableSetOf<WindowInsetsAnimationCompat>()
  private var preparedIme: WindowInsetsAnimationCompat? = null
  private var snapshot: WindowSnapshot? = null
  private var wallpaperToken = 0L
  private val backdrop = WindowBackdrop()
  private val statusBarAppearance = AndroidStatusBarAppearance(window, mainHandler)

  fun onCreate() {
    WindowCompat.enableEdgeToEdge(window)
    window.decorView.background = backdrop
    configureSystemBars()
    refresh()
  }

  fun onConfigurationChanged() {
    configureSystemBars()
    refresh()
  }

  fun onResume() {
    configureSystemBars()
    statusBarAppearance.start()
    refresh()
  }

  fun onPause() {
    statusBarAppearance.stop()
  }

  fun onWebViewAvailable() {
    val webView = webViewProvider() ?: return
    // Wry installs the WebView in setContentView after onWebViewCreate returns.
    webView.addOnAttachStateChangeListener(object : View.OnAttachStateChangeListener {
      override fun onViewAttachedToWindow(view: View) { refresh() }
      override fun onViewDetachedFromWindow(view: View) {}
    })
    ViewCompat.setOnApplyWindowInsetsListener(webView) { view, insets ->
      val handled = WindowInsetsCompat.Type.systemBars() or
        WindowInsetsCompat.Type.displayCutout() or WindowInsetsCompat.Type.ime()
      val zeroed = WindowInsetsCompat.Builder(insets)
        .setInsets(handled, Insets.NONE)
        // IME has no ignoring-visibility inset; requesting it throws.
        .setInsetsIgnoringVisibility(handled and WindowInsetsCompat.Type.ime().inv(), Insets.NONE)
        .setVisible(handled, false)
        .build()
      // API 31+ forwards these zeros to Chromium. On API 28–30 our listener
      // replaces Chromium's constructor listener, opting out of its inset handling.
      ViewCompat.onApplyWindowInsets(view, zeroed)
    }
    refresh()
  }

  fun setImmersiveFullscreenEnabled(enabled: Boolean) {
    immersiveFullscreenEnabled = enabled
    configureSystemBars()
    refresh()
  }

  fun isImmersiveFullscreenEnabled() = immersiveFullscreenEnabled

  fun setElementFullscreen(enabled: Boolean) {
    elementFullscreen = enabled
    configureSystemBars()
  }

  fun getSnapshot(): JSONObject = checkNotNull(snapshot) { "Window layout is not ready" }.toJson()

  fun beginBackdrop(windowRevision: Long, color: Int, replaceWallpaper: Boolean): JSONObject? {
    if (windowRevision != snapshot?.revision) return null
    backdrop.color = color
    if (replaceWallpaper) {
      wallpaperToken++
      backdrop.strips = emptyList()
    }
    backdrop.invalidateSelf()
    return JSONObject().put("window", getSnapshot()).put("token", wallpaperToken)
  }

  fun applyBackdrop(windowRevision: Long, token: Long, strips: List<WindowBackdropStrip>) {
    // Superseded renders are normal; they must not replace the currently selected wallpaper.
    if (windowRevision != snapshot?.revision || token != wallpaperToken) return
    backdrop.strips = strips
    backdrop.invalidateSelf()
  }

  private fun configureSystemBars() {
    val dark = (resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK) ==
      Configuration.UI_MODE_NIGHT_YES
    WindowInsetsControllerCompat(window, window.decorView).apply {
      isAppearanceLightStatusBars = statusBarAppearance.useDarkIcons ?: !dark
      isAppearanceLightNavigationBars = !dark
      systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
      if (immersiveFullscreenEnabled || elementFullscreen) hide(WindowInsetsCompat.Type.systemBars())
      else show(WindowInsetsCompat.Type.systemBars())
    }
  }

  private fun refresh() {
    val root = contentRootProvider() ?: return
    if (attachedRoot !== root) installInsetsCallbacks(root)
    (ViewCompat.getRootWindowInsets(root) ?: targetInsets)?.let(::acceptInsets)
    ViewCompat.requestApplyInsets(root)
  }

  private fun acceptInsets(insets: WindowInsetsCompat) {
    targetInsets = insets
    updateSnapshot(insets)
    if (preparedIme == null && animations.isEmpty()) layoutContent(insets)
  }

  private fun installInsetsCallbacks(root: ViewGroup) {
    attachedRoot = root
    ViewGroupCompat.installCompatInsetsDispatch(root)
    ViewCompat.setOnApplyWindowInsetsListener(root) { _, insets ->
      acceptInsets(insets)
      // Siblings (including element fullscreen) keep the original facts.
      insets
    }
    ViewCompat.setWindowInsetsAnimationCallback(root, object : WindowInsetsAnimationCompat.Callback(DISPATCH_MODE_CONTINUE_ON_SUBTREE) {
      override fun onPrepare(animation: WindowInsetsAnimationCompat) {
        if (animation.typeMask and WindowInsetsCompat.Type.ime() == 0) return
        preparedIme = animation
        // API 30 sends no onEnd when cancellation precedes onStart. Preparation
        // lasts one layout pass, never the lifetime of an unstarted animation.
        OneShotPreDrawListener.add(root) {
          root.post {
            if (preparedIme === animation) {
              preparedIme = null
              (ViewCompat.getRootWindowInsets(root) ?: targetInsets)?.let(::acceptInsets)
            }
          }
        }
      }
      override fun onStart(
        animation: WindowInsetsAnimationCompat,
        bounds: WindowInsetsAnimationCompat.BoundsCompat,
      ): WindowInsetsAnimationCompat.BoundsCompat {
        if (animation.typeMask and WindowInsetsCompat.Type.ime() != 0) {
          animations.add(animation)
          if (preparedIme === animation) preparedIme = null
        }
        return bounds
      }
      override fun onProgress(
        insets: WindowInsetsCompat,
        runningAnimations: MutableList<WindowInsetsAnimationCompat>,
      ): WindowInsetsCompat {
        if (runningAnimations.any { it.typeMask and WindowInsetsCompat.Type.ime() != 0 }) {
          layoutContent(insets)
        }
        return insets
      }
      override fun onEnd(animation: WindowInsetsAnimationCompat) {
        if (animation.typeMask and WindowInsetsCompat.Type.ime() == 0) return
        if (preparedIme === animation) preparedIme = null
        animations.remove(animation)
        (ViewCompat.getRootWindowInsets(root) ?: targetInsets)?.let(::acceptInsets)
      }
    })
    // Rotation and window resizing update the snapshot even when insets stay unchanged.
    root.addOnLayoutChangeListener { _, _, _, _, _, _, _, _, _ ->
      targetInsets?.let(::acceptInsets)
    }
  }

  private fun policyInsets(insets: WindowInsetsCompat): Insets =
    if (immersiveFullscreenEnabled) Insets.NONE
    else insets.getInsetsIgnoringVisibility(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout())

  private fun updateSnapshot(insets: WindowInsetsCompat) {
    val decor = window.decorView
    if (decor.width == 0 || decor.height == 0) return
    val safe = policyInsets(insets)
    val previousRevision = snapshot?.revision ?: 0L
    val next = WindowSnapshot(
      previousRevision, decor.width, decor.height, resources.displayMetrics.density, safe,
    )
    if (next == snapshot) return
    snapshot = next.copy(revision = previousRevision + 1)
    backdrop.strips = emptyList()
    backdrop.invalidateSelf()
    webViewProvider()?.evaluateJavascript(
      "window.dispatchEvent(new CustomEvent('tt-window-changed',{detail:${getSnapshot()}}));", null)
  }

  private fun layoutContent(insets: WindowInsetsCompat) {
    val webView = webViewProvider() ?: return
    val parent = webView.parent as? ViewGroup ?: return
    if (parent.width == 0 || parent.height == 0) return
    val params = webView.layoutParams as ViewGroup.MarginLayoutParams
    val safe = policyInsets(insets)
    val location = IntArray(2)
    parent.getLocationInWindow(location)
    val decor = window.decorView
    val left = maxOf(0, safe.left - location[0])
    val top = maxOf(0, safe.top - location[1])
    val right = minOf(parent.width, decor.width - safe.right - location[0])
    // Intersect the usable window rectangle with the parent in window coordinates.
    val bottomOcclusion = maxOf(safe.bottom, insets.getInsets(WindowInsetsCompat.Type.ime()).bottom)
    val bottom = minOf(parent.height, decor.height - bottomOcclusion - location[1])
    val width = maxOf(0, right - left)
    val height = maxOf(0, bottom - top)
    if (params.leftMargin == left && params.topMargin == top &&
      params.width == width && params.height == height) return
    params.leftMargin = left
    params.topMargin = top
    params.rightMargin = 0
    params.bottomMargin = 0
    params.width = width
    params.height = height
    webView.layoutParams = params
  }
}

data class WindowBackdropStrip(val x: Int, val y: Int, val bitmap: Bitmap)

private class WindowBackdrop : Drawable() {
  var color = Color.BLACK
  var strips: List<WindowBackdropStrip> = emptyList()
  private val paint = Paint()
  override fun draw(canvas: Canvas) {
    canvas.drawColor(color)
    for (strip in strips) canvas.drawBitmap(strip.bitmap, strip.x.toFloat(), strip.y.toFloat(), paint)
  }
  override fun setAlpha(alpha: Int) {}
  override fun setColorFilter(colorFilter: ColorFilter?) {}
  @Deprecated("Deprecated in Android")
  override fun getOpacity() = PixelFormat.OPAQUE
}

private data class WindowSnapshot(
  val revision: Long,
  val width: Int,
  val height: Int,
  val scale: Float,
  val insets: Insets,
) {
  fun toJson(): JSONObject = JSONObject()
    .put("revision", revision)
    .put("width", width)
    .put("height", height)
    .put("scale", scale.toDouble())
    .put("insets", JSONObject()
      .put("top", insets.top).put("right", insets.right)
      .put("bottom", insets.bottom).put("left", insets.left))
}
