package com.tauritavern.client

import android.app.DownloadManager
import android.content.Context
import android.net.Uri
import android.os.Environment
import android.os.Handler
import android.os.Looper
import android.util.Base64
import android.util.Log
import android.webkit.JavascriptInterface
import android.webkit.WebView
import android.widget.Toast
import org.json.JSONObject
import java.io.File

/**
 * Adds a long-press "保存图片" entry for images rendered inside the WebView.
 *
 * One rule decides the path: the system downloader can only reach real network
 * locations, so anything that exists solely inside this WebView — `blob:`,
 * `data:` and the app's own loopback origin — is read out of the page instead.
 * Those payloads are published through [publicDownloads], which already owns the
 * MediaStore hand-off.
 */
class AndroidImageContextMenuInstaller(
  private val context: Context,
  private val publicDownloads: AndroidPublicDownloadJsBridge,
  private val stagingRoot: File,
) {
  private val mainHandler = Handler(Looper.getMainLooper())

  fun install(webView: WebView) {
    webView.addJavascriptInterface(this, INTERFACE_NAME)
    webView.isLongClickable = true
    webView.setOnCreateContextMenuListener { menu, _, _ ->
      val imageSource = webView.hitTestResult.imageSource()
      if (imageSource.isEmpty()) {
        return@setOnCreateContextMenuListener
      }

      menu
        .add(SAVE_MENU_TITLE)
        .setOnMenuItemClickListener {
          save(webView, imageSource)
          true
        }
    }
  }

  private fun save(webView: WebView, imageSource: String) {
    val pageSource = pageFetchSourceOf(imageSource)
    if (pageSource == null) {
      enqueueNetworkDownload(imageSource)
    } else {
      readThroughWebView(webView, pageSource)
    }
  }

  /**
   * Returns what the page should fetch, or null when the system downloader can
   * serve the URL on its own. App assets resolve relatively so the request stays
   * same-origin; every other non-network source is handed over untouched.
   */
  private fun pageFetchSourceOf(url: String): String? {
    if (!url.startsWith("http://") && !url.startsWith("https://")) {
      return url
    }

    val uri = Uri.parse(url)
    val host = uri.host.orEmpty()
    val isLoopback = host == "localhost" || host == "127.0.0.1" || host == "::1" || host.endsWith(".localhost")
    if (!isLoopback) {
      return null
    }

    val path = uri.encodedPath.orEmpty().ifEmpty { "/" }
    return path + uri.encodedQuery?.let { "?$it" }.orEmpty()
  }

  private fun enqueueNetworkDownload(url: String) {
    val fileName = imageDownloadName(url)
    val request =
      DownloadManager
        .Request(Uri.parse(url))
        .apply {
          setTitle(fileName)
          setDestinationInExternalPublicDir(Environment.DIRECTORY_DOWNLOADS, fileName)
          setNotificationVisibility(DownloadManager.Request.VISIBILITY_VISIBLE_NOTIFY_COMPLETED)
        }

    val outcome = runCatching { context.getSystemService(DownloadManager::class.java)?.enqueue(request) }
    if (outcome.getOrNull() != null) {
      toast("开始下载：$fileName")
    } else {
      fail("无法开始下载", outcome.exceptionOrNull())
    }
  }

  /**
   * The page resolves the URL and pushes a data URL back through
   * [deliverImageDataUrl]. A bridge call is used rather than an
   * `evaluateJavascript` result because a returned Promise is not reliably
   * awaited across WebView versions. The payload still crosses IPC in one piece,
   * so very large images remain the known limit of this path.
   */
  private fun readThroughWebView(webView: WebView, source: String) {
    val script =
      "fetch(${JSONObject.quote(source)})" +
        ".then(r=>r.blob())" +
        ".then(b=>new Promise(ok=>{" +
        "const f=new FileReader();" +
        "f.onload=()=>${INTERFACE_NAME}.deliverImageDataUrl(f.result);" +
        "f.readAsDataURL(b)" +
        "}))" +
        ".catch(e=>${INTERFACE_NAME}.reportFailure(String(e)))"

    webView.evaluateJavascript(script, null)
  }

  @JavascriptInterface
  fun deliverImageDataUrl(dataUrl: String?) {
    if (dataUrl.isNullOrBlank()) {
      fail("页面没有返回图片数据", null)
      return
    }

    Log.i(LOG_TAG, "page delivered ${dataUrl.length} chars")
    mainHandler.post { saveDataUrl(dataUrl) }
  }

  @JavascriptInterface
  fun reportFailure(reason: String?) {
    fail("保存失败", IllegalStateException(reason ?: "unknown"))
  }

  private fun saveDataUrl(dataUrl: String) {
    val payload = dataUrl.substringAfter(',', "")
    val mimeType =
      dataUrl
        .substringBefore(';')
        .removePrefix(DATA_URL_PREFIX)
        .ifBlank { DEFAULT_MIME_TYPE }

    val bytes = runCatching { Base64.decode(payload, Base64.DEFAULT) }.getOrNull()
    if (bytes == null || bytes.isEmpty()) {
      fail("图片数据无法解码", null)
      return
    }

    val fileName = "image_${System.currentTimeMillis()}.${mimeType.substringAfter('/')}"
    val stagingFile = File(stagingRoot, fileName)

    runCatching {
      stagingFile.parentFile?.mkdirs()
      stagingFile.writeBytes(bytes)
      publicDownloads.saveFileToDownloads(stagingFile.absolutePath, fileName, mimeType)
    }.onSuccess {
      toast("已保存到下载：$fileName")
    }.onFailure {
      fail("保存失败", it)
    }

    // The staging copy exists only to hand bytes to the MediaStore bridge.
    stagingFile.delete()
  }

  /** Reports a failure to the caller instead of leaving it silent in logcat. */
  private fun fail(message: String, cause: Throwable?) {
    Log.w(LOG_TAG, message, cause)
    mainHandler.post { toast(message) }
  }

  private fun toast(message: String) {
    Toast.makeText(context, message, Toast.LENGTH_SHORT).show()
  }

  private fun WebView.HitTestResult.imageSource(): String =
    when (type) {
      WebView.HitTestResult.IMAGE_TYPE,
      WebView.HitTestResult.SRC_IMAGE_ANCHOR_TYPE,
      -> extra.orEmpty()

      else -> ""
    }

  private fun imageDownloadName(url: String): String {
    val segment = Uri.parse(url).lastPathSegment.orEmpty()
    val sanitized =
      segment
        .map { if (it.isLetterOrDigit() || it in ".-_") it else '_' }
        .joinToString("")

    return sanitized.ifBlank { "image" }
  }

  companion object {
    private const val LOG_TAG = "TauriTavernImageMenu"
    private const val INTERFACE_NAME = "TauriTavernImageMenuBridge"
    private const val SAVE_MENU_TITLE = "保存图片"
    private const val DATA_URL_PREFIX = "data:"
    private const val DEFAULT_MIME_TYPE = "image/png"
  }
}