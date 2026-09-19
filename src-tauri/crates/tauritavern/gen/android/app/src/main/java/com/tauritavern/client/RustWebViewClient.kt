// Local override of Wry's generated RustWebViewClient.
// Baseline: wry 0.55.1. Local deltas: main-frame listener, intercept failure response,
// and preserving an explicit host Cache-Control policy.

@file:Suppress("RedundantOverride")

package com.tauritavern.client

import android.content.Context
import android.graphics.Bitmap
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.util.Log
import android.webkit.WebResourceError
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.webkit.WebViewAssetLoader
import java.io.ByteArrayInputStream

class RustWebViewClient(
  webView: RustWebView,
  context: Context,
) : WebViewClient() {
  private val interceptedState = mutableMapOf<String, Boolean>()
  var currentUrl: String = "about:blank"
  private var lastInterceptedUrl: Uri? = null
  private var pendingUrlRedirect: String? = null

  interface MainFrameNavigationListener {
    fun onMainFramePageStarted(view: WebView, url: String)
  }

  private val assetLoader =
    WebViewAssetLoader
      .Builder()
      .setDomain(Rust.assetLoaderDomain(webView.id))
      .addPathHandler("/", WebViewAssetLoader.AssetsPathHandler(context))
      .build()

  override fun shouldInterceptRequest(
    view: WebView,
    request: WebResourceRequest,
  ): WebResourceResponse? {
    pendingUrlRedirect?.let {
      Handler(Looper.getMainLooper()).post { view.loadUrl(it) }
      pendingUrlRedirect = null
      return null
    }

    lastInterceptedUrl = request.url
    return if (Rust.withAssetLoader((view as RustWebView).id)) {
      assetLoader.shouldInterceptRequest(request.url)
    } else {
      val requestUrl = request.url.toString()

      val response =
        try {
          Rust.handleRequest(view.id, request, view.isDocumentStartScriptEnabled)
        } catch (throwable: Throwable) {
          logInterceptFailure(requestUrl, throwable)
          buildErrorResponse(throwable)
        }

      applyDefaultCachePolicy(response)
      interceptedState[requestUrl] = response != null
      response
    }
  }

  override fun shouldOverrideUrlLoading(
    view: WebView,
    request: WebResourceRequest,
  ): Boolean {
    return Rust.shouldOverride((view as RustWebView).id, request.url.toString())
  }

  override fun onPageStarted(
    view: WebView,
    url: String,
    favicon: Bitmap?,
  ) {
    currentUrl = url
    mainFrameNavigationListener?.onMainFramePageStarted(view, url)
    if (interceptedState[url] == false) {
      val webView = view as RustWebView
      for (script in webView.initScripts) {
        view.evaluateJavascript(script, null)
      }
    }
    return Rust.onPageLoading((view as RustWebView).id, url)
  }

  override fun onPageFinished(view: WebView, url: String) {
    Rust.onPageLoaded((view as RustWebView).id, url)
  }

  override fun onReceivedError(
    view: WebView,
    request: WebResourceRequest,
    error: WebResourceError,
  ) {
    // we get a net::ERR_CONNECTION_REFUSED when an external URL redirects to a custom protocol
    // e.g. oauth flow, because shouldInterceptRequest is not called on redirects
    // so we must force retry here with loadUrl() to get a chance of the custom protocol to kick in
    if (error.errorCode == ERROR_CONNECT && request.isForMainFrame && request.url != lastInterceptedUrl) {
      // prevent the default error page from showing
      view.stopLoading()
      // without this initial loadUrl the app is stuck
      view.loadUrl(request.url.toString())
      // ensure the URL is actually loaded - for some reason there's a race condition and we need to call loadUrl() again later
      pendingUrlRedirect = request.url.toString()
    } else {
      super.onReceivedError(view, request, error)
    }
  }

  companion object {
    private const val LOG_TAG = "TauriTavern/WebView"

    @Volatile
    var mainFrameNavigationListener: MainFrameNavigationListener? = null

    private fun logInterceptFailure(url: String, throwable: Throwable) {
      try {
        val runtime = Runtime.getRuntime()
        val mem =
          "mem(total=${runtime.totalMemory()}, free=${runtime.freeMemory()}, max=${runtime.maxMemory()})"
        Log.e(
          LOG_TAG,
          "shouldInterceptRequest failed for $url ($mem): ${throwable.javaClass.name}: ${throwable.message}",
          throwable,
        )
      } catch (_: Throwable) {
        // Ignore logging failures under low memory pressure.
      }
    }

    private fun buildErrorResponse(throwable: Throwable): WebResourceResponse? {
      val message =
        if (throwable is OutOfMemoryError) {
          "Out of memory while serving resource."
        } else {
          "Failed to serve resource."
        }

      return try {
        WebResourceResponse(
          "text/plain",
          "utf-8",
          ByteArrayInputStream(message.toByteArray(Charsets.UTF_8)),
        )
          .apply {
            setStatusCodeAndReasonPhrase(500, "Internal Server Error")
            responseHeaders =
              mapOf(
                "Cache-Control" to "no-store",
              )
          }
      } catch (_: Throwable) {
        // Fall back to default WebView behavior if we cannot even build an error response.
        null
      }
    }

    private fun applyDefaultCachePolicy(response: WebResourceResponse?) {
      if (response == null) {
        return
      }

      val headers = response.responseHeaders
      if (headers == null) {
        response.responseHeaders = mapOf("Cache-Control" to "no-store")
      } else if (headers.keys.none { it.equals("Cache-Control", ignoreCase = true) }) {
        headers["Cache-Control"] = "no-store"
      }
    }
  }
}
