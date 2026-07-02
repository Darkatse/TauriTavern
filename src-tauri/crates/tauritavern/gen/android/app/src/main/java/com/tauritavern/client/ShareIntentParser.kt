package com.tauritavern.client

import android.content.ContentResolver
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Parcelable
import android.provider.OpenableColumns
import android.util.Log
import org.json.JSONObject
import java.io.File
import java.util.Locale

class ShareIntentParser(
  private val contentResolver: ContentResolver,
  private val cacheDir: File,
) {
  fun canHandle(intent: Intent?): Boolean {
    val action = intent?.action ?: return false
    return action == Intent.ACTION_SEND || action == Intent.ACTION_SEND_MULTIPLE
  }

  fun parse(intent: Intent): List<NativeSharePayload> {
    return when (intent.action) {
      Intent.ACTION_SEND -> parseSingleSharePayload(intent)?.let { listOf(it) } ?: emptyList()
      Intent.ACTION_SEND_MULTIPLE -> parseMultipleSharePayloads(intent)
      else -> emptyList()
    }
  }

  private fun parseSingleSharePayload(intent: Intent): NativeSharePayload? {
    val streamUri = intent.getParcelableExtraCompat<Uri>(Intent.EXTRA_STREAM)
    if (streamUri != null) {
      return createPngSharePayload(streamUri, intent.type)
    }

    return createUrlSharePayload(intent)
  }

  private fun parseMultipleSharePayloads(intent: Intent): List<NativeSharePayload> {
    val payloads = mutableListOf<NativeSharePayload>()
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

  private fun createUrlSharePayload(intent: Intent): NativeSharePayload? {
    val rawText =
      intent.getStringExtra(Intent.EXTRA_TEXT)
        ?: intent.getCharSequenceExtra(Intent.EXTRA_TEXT)?.toString()
        ?: return null

    val url = extractFirstHttpUrl(rawText) ?: return null
    return NativeSharePayload(kind = "url", url = url)
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

  private fun createPngSharePayload(uri: Uri, mimeTypeHint: String?): NativeSharePayload? {
    val displayName = queryDisplayName(uri) ?: uri.lastPathSegment ?: "shared-character.png"
    val resolvedMimeType = mimeTypeHint ?: contentResolver.getType(uri)
    val isPngFile =
      isPngMimeType(resolvedMimeType) || displayName.lowercase(Locale.US).endsWith(".png")

    if (!isPngFile) {
      return null
    }

    val copiedFile = copySharedUriToCache(uri, displayName) ?: return null
    return NativeSharePayload(
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
    private const val TAG = "ShareIntentParser"
    private val HTTP_URL_REGEX = Regex("""https?://[^\s]+""", RegexOption.IGNORE_CASE)
  }
}

data class NativeSharePayload(
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
