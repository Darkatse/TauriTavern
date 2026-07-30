package com.tauritavern.client

import android.graphics.Color
import android.view.View
import android.view.ViewGroup
import android.webkit.WebChromeClient
import android.widget.FrameLayout

interface AndroidWebFullscreenHost {
  fun showWebFullscreenView(view: View, callback: WebChromeClient.CustomViewCallback): Boolean

  fun hideWebFullscreenView(): Boolean
}

class AndroidWebFullscreenController(
  private val contentRootProvider: () -> ViewGroup?,
  private val insetsBridge: AndroidInsetsBridge,
) {
  private var fullscreenContainer: FrameLayout? = null
  private var fullscreenCallback: WebChromeClient.CustomViewCallback? = null
  private var immersiveFullscreenBeforeShow: Boolean? = null

  fun show(view: View, callback: WebChromeClient.CustomViewCallback): Boolean {
    hide()

    val contentRoot = contentRootProvider() ?: return false

    val container =
      FrameLayout(contentRoot.context).apply {
        setBackgroundColor(Color.BLACK)
        layoutParams =
          ViewGroup.LayoutParams(
            ViewGroup.LayoutParams.MATCH_PARENT,
            ViewGroup.LayoutParams.MATCH_PARENT,
          )
        addView(
          view,
          FrameLayout.LayoutParams(
            ViewGroup.LayoutParams.MATCH_PARENT,
            ViewGroup.LayoutParams.MATCH_PARENT,
          ),
        )
      }

    immersiveFullscreenBeforeShow = insetsBridge.isImmersiveFullscreenEnabled()
    insetsBridge.setImmersiveFullscreenEnabled(true)

    contentRoot.addView(container)
    container.bringToFront()

    fullscreenContainer = container
    fullscreenCallback = callback
    return true
  }

  fun hide(): Boolean {
    val container = fullscreenContainer ?: return false
    fullscreenContainer = null

    container.removeAllViews()
    (container.parent as? ViewGroup)?.removeView(container)

    val callback = fullscreenCallback
    fullscreenCallback = null

    immersiveFullscreenBeforeShow?.let { insetsBridge.setImmersiveFullscreenEnabled(it) }
    immersiveFullscreenBeforeShow = null

    callback?.onCustomViewHidden()
    return true
  }
}
