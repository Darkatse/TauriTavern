package com.tauritavern.client

import android.os.Handler
import android.webkit.WebView
import org.json.JSONArray

class SharePayloadDispatcher(
  private val webViewProvider: () -> WebView?,
  private val isDestroyed: () -> Boolean,
  private val mainHandler: Handler,
  private val readinessPoller: WebViewReadinessPoller,
) {
  private val pendingSharePayloads = ArrayDeque<NativeSharePayload>()
  private var isShareBridgeSyncScheduled: Boolean = false

  fun enqueue(payloads: Collection<NativeSharePayload>) {
    if (payloads.isEmpty()) {
      return
    }

    pendingSharePayloads.addAll(payloads)
    scheduleShareBridgeSyncWhenReady()
  }

  fun requestDispatch() {
    if (pendingSharePayloads.isEmpty()) {
      return
    }

    scheduleShareBridgeSyncWhenReady()
  }

  private fun scheduleShareBridgeSyncWhenReady() {
    if (webViewProvider() == null) {
      return
    }
    if (isShareBridgeSyncScheduled) {
      return
    }

    isShareBridgeSyncScheduled = true
    readinessPoller.pollUntilReady(
      readinessScript = SHARE_BRIDGE_READY_SCRIPT,
      onReady = { flushPendingSharePayloads() },
      onFinished = {
        isShareBridgeSyncScheduled = false
        if (pendingSharePayloads.isNotEmpty()) {
          mainHandler.postDelayed(
            { if (!isDestroyed()) scheduleShareBridgeSyncWhenReady() },
            WebViewReadinessPoller.DEFAULT_RETRY_DELAY_MS,
          )
        }
      },
    )
  }

  private fun flushPendingSharePayloads() {
    val targetWebView = webViewProvider() ?: return
    if (pendingSharePayloads.isEmpty()) {
      return
    }

    val payloads = mutableListOf<NativeSharePayload>()
    while (pendingSharePayloads.isNotEmpty()) {
      payloads.add(pendingSharePayloads.removeFirst())
    }

    val payloadArray = JSONArray()
    for (payload in payloads) {
      payloadArray.put(payload.toJsonObject())
    }

    val script =
      """
      (() => {
        const bridge = window.__TAURITAVERN_NATIVE_SHARE__;
        if (!bridge || typeof bridge.push !== 'function') return;
        const payloads = $payloadArray;
        for (const payload of payloads) {
          bridge.push(payload);
        }
      })();
      """.trimIndent()

    targetWebView.post {
      val activeWebView = webViewProvider()
      if (activeWebView == null || isDestroyed()) {
        requeueAtFront(payloads)
        if (!isDestroyed()) {
          mainHandler.postDelayed(
            { scheduleShareBridgeSyncWhenReady() },
            WebViewReadinessPoller.DEFAULT_RETRY_DELAY_MS,
          )
        }
        return@post
      }

      activeWebView.evaluateJavascript(script, null)
    }
  }

  private fun requeueAtFront(payloads: List<NativeSharePayload>) {
    for (index in payloads.indices.reversed()) {
      pendingSharePayloads.addFirst(payloads[index])
    }
  }

  companion object {
    private val SHARE_BRIDGE_READY_SCRIPT =
      """
      (() => (
        document.readyState !== 'loading'
        && location.href !== 'about:blank'
        && !!window.__TAURITAVERN_NATIVE_SHARE__
        && typeof window.__TAURITAVERN_NATIVE_SHARE__.push === 'function'
      ))();
      """.trimIndent()
  }
}
