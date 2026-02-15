package com.tauritavern.client

import android.content.Intent
import android.content.res.Configuration
import android.graphics.Color
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Parcelable
import android.provider.OpenableColumns
import android.util.Log
import android.view.View
import android.view.WindowManager
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge
import androidx.core.graphics.Insets
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import org.json.JSONObject
import java.io.File
import java.util.Locale

class MainActivity : TauriActivity() {
  private var webView: WebView? = null
  private var systemBarInsets: Insets = Insets.NONE
  private val pendingSharePayloads = ArrayDeque<SharePayload>()

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    configureImmersiveSystemBars()
    attachSystemInsetsListener()
    requestSystemInsets()
    captureShareIntent(intent)
  }

  override fun onNewIntent(intent: Intent) {
    super.onNewIntent(intent)
    setIntent(intent)
    captureShareIntent(intent)
  }

  override fun onConfigurationChanged(newConfig: Configuration) {
    super.onConfigurationChanged(newConfig)
    configureImmersiveSystemBars()
    refreshInsetsInjection()
  }

  override fun onWebViewCreate(webView: WebView) {
    this.webView = webView
    refreshInsetsInjection()
    dispatchPendingSharePayloads()
  }

  override fun onResume() {
    super.onResume()
    refreshInsetsInjection()
    dispatchPendingSharePayloads()
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

    fun toCssPx(value: Int): String = String.format(Locale.US, "%.2fpx", value / density)

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

    targetWebView.post { targetWebView.evaluateJavascript(script, null) }
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
      targetWebView.evaluateJavascript(readinessScript) { value ->
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

  private fun captureShareIntent(intent: Intent?) {
    val payloads = parseShareIntent(intent)
    if (payloads.isEmpty()) {
      return
    }

    pendingSharePayloads.addAll(payloads)
    dispatchPendingSharePayloads()
  }

  private fun parseShareIntent(intent: Intent?): List<SharePayload> {
    val incomingIntent = intent ?: return emptyList()
    return when (incomingIntent.action) {
      Intent.ACTION_SEND ->
        parseSingleSharePayload(incomingIntent)?.let { listOf(it) } ?: emptyList()
      Intent.ACTION_SEND_MULTIPLE -> parseMultipleSharePayloads(incomingIntent)
      else -> emptyList()
    }
  }

  private fun parseSingleSharePayload(intent: Intent): SharePayload? {
    val streamUri = intent.getParcelableExtraCompat<Uri>(Intent.EXTRA_STREAM)
    if (streamUri != null) {
      return createPngSharePayload(streamUri, intent.type)
    }

    return createUrlSharePayload(intent)
  }

  private fun parseMultipleSharePayloads(intent: Intent): List<SharePayload> {
    val payloads = mutableListOf<SharePayload>()
    val streamUris = intent.getParcelableArrayListExtraCompat<Uri>(Intent.EXTRA_STREAM).orEmpty()
    for (uri in streamUris) {
      createPngSharePayload(uri, intent.type)?.let { payloads.add(it) }
    }

    if (payloads.isNotEmpty()) {
      return payloads
    }

    createUrlSharePayload(intent)?.let { payloads.add(it) }
    return payloads
  }

  private fun createUrlSharePayload(intent: Intent): SharePayload? {
    val rawText =
      intent.getStringExtra(Intent.EXTRA_TEXT)
        ?: intent.getCharSequenceExtra(Intent.EXTRA_TEXT)?.toString()
        ?: return null

    val url = extractFirstHttpUrl(rawText) ?: return null
    return SharePayload(kind = "url", url = url)
  }

  private fun extractFirstHttpUrl(input: String): String? {
    val match = HTTP_URL_REGEX.find(input) ?: return null
    val candidate = match.value.trim().trimEnd('.', ',', ';', ':', ')', ']', '}', '>', '"', '\'')
    return normalizeHttpUrl(candidate)
  }

  private fun normalizeHttpUrl(url: String): String? {
    return try {
      val parsed = Uri.parse(url)
      val scheme = parsed.scheme?.lowercase(Locale.US)
      if ((scheme == "http" || scheme == "https") && !parsed.host.isNullOrBlank()) {
        parsed.toString()
      } else {
        null
      }
    } catch (_: Exception) {
      null
    }
  }

  private fun createPngSharePayload(uri: Uri, mimeTypeHint: String?): SharePayload? {
    val displayName = queryDisplayName(uri) ?: uri.lastPathSegment ?: "shared-character.png"
    val resolvedMimeType = mimeTypeHint ?: contentResolver.getType(uri)
    val isPngFile =
      isPngMimeType(resolvedMimeType) || displayName.lowercase(Locale.US).endsWith(".png")

    if (!isPngFile) {
      return null
    }

    val copiedFile = copySharedUriToCache(uri, displayName) ?: return null
    return SharePayload(
      kind = "png",
      path = copiedFile.absolutePath,
      fileName = copiedFile.name,
      mimeType = "image/png",
    )
  }

  private fun isPngMimeType(mimeType: String?): Boolean {
    return (mimeType ?: "").lowercase(Locale.US).startsWith("image/png")
  }

  private fun queryDisplayName(uri: Uri): String? {
    return try {
      contentResolver
        .query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)
        ?.use { cursor ->
          val nameIndex = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
          if (nameIndex < 0 || !cursor.moveToFirst()) {
            return@use null
          }

          cursor.getString(nameIndex)
        }
    } catch (_: Exception) {
      null
    }
  }

  private fun copySharedUriToCache(uri: Uri, originalName: String): File? {
    return try {
      val shareDir = File(cacheDir, "share-target-imports").apply { mkdirs() }
      val safeName = sanitizeFileName(originalName)
      val targetFile = createUniqueFile(shareDir, safeName)

      contentResolver.openInputStream(uri)?.use { input ->
        targetFile.outputStream().use { output -> input.copyTo(output) }
      } ?: return null

      targetFile
    } catch (error: Exception) {
      Log.e(TAG, "Failed to persist shared PNG", error)
      null
    }
  }

  private fun sanitizeFileName(fileName: String): String {
    val sanitized =
      fileName
        .replace(Regex("[\\\\/:*?\"<>|\\u0000-\\u001f]"), "_")
        .trim()
        .trimEnd('.', ' ')
        .ifBlank { "shared-character.png" }

    return if (sanitized.lowercase(Locale.US).endsWith(".png")) sanitized else "$sanitized.png"
  }

  private fun createUniqueFile(directory: File, fileName: String): File {
    var candidate = File(directory, fileName)
    if (!candidate.exists()) {
      return candidate
    }

    val stem = candidate.nameWithoutExtension.ifBlank { "shared-character" }
    val extension = candidate.extension.ifBlank { "png" }
    var index = 1

    while (candidate.exists()) {
      candidate = File(directory, "$stem-$index.$extension")
      index += 1
    }

    return candidate
  }

  private fun dispatchPendingSharePayloads() {
    if (pendingSharePayloads.isEmpty()) {
      return
    }

    syncShareBridgeWhenPageReady()
  }

  private fun syncShareBridgeWhenPageReady(attempt: Int = 0) {
    val targetWebView = webView ?: return
    val maxAttempts = 100
    val retryDelayMs = 80L

    val readinessScript =
      """
      (() => (
        document.readyState !== 'loading'
        && location.href !== 'about:blank'
        && !!window.__TAURITAVERN_NATIVE_SHARE__
        && typeof window.__TAURITAVERN_NATIVE_SHARE__.push === 'function'
      ))();
      """.trimIndent()

    targetWebView.post {
      targetWebView.evaluateJavascript(readinessScript) { value ->
        if (value == "true") {
          flushPendingSharePayloads()
          return@evaluateJavascript
        }

        if (attempt < maxAttempts) {
          targetWebView.postDelayed({ syncShareBridgeWhenPageReady(attempt + 1) }, retryDelayMs)
        }
      }
    }
  }

  private fun flushPendingSharePayloads() {
    val targetWebView = webView ?: return
    if (pendingSharePayloads.isEmpty()) {
      return
    }

    val payloads = mutableListOf<SharePayload>()
    while (pendingSharePayloads.isNotEmpty()) {
      payloads.add(pendingSharePayloads.removeFirst())
    }

    targetWebView.post {
      for (payload in payloads) {
        val script =
          "(() => { window.__TAURITAVERN_NATIVE_SHARE__.push(${payload.toJsonObject()}); })();"
        targetWebView.evaluateJavascript(script, null)
      }
    }
  }

  private data class SharePayload(
    val kind: String,
    val url: String? = null,
    val path: String? = null,
    val fileName: String? = null,
    val mimeType: String? = null,
  ) {
    fun toJsonObject(): JSONObject {
      return JSONObject().apply {
        put("kind", kind)
        url?.let { put("url", it) }
        path?.let { put("path", it) }
        fileName?.let { put("fileName", it) }
        mimeType?.let { put("mimeType", it) }
      }
    }
  }

  private inline fun <reified T : Parcelable> Intent.getParcelableExtraCompat(name: String): T? {
    return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
      getParcelableExtra(name, T::class.java)
    } else {
      @Suppress("DEPRECATION")
      getParcelableExtra(name)
    }
  }

  private inline fun <reified T : Parcelable> Intent.getParcelableArrayListExtraCompat(
    name: String
  ): ArrayList<T>? {
    return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
      getParcelableArrayListExtra(name, T::class.java)
    } else {
      @Suppress("DEPRECATION")
      getParcelableArrayListExtra(name)
    }
  }

  companion object {
    private const val TAG = "MainActivity"
    private val HTTP_URL_REGEX = Regex("""https?://[^\s]+""", RegexOption.IGNORE_CASE)
  }
}
