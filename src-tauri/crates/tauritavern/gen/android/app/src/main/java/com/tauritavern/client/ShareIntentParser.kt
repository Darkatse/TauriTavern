package com.tauritavern.client

import android.content.ContentResolver
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Parcelable
import android.provider.OpenableColumns
import org.json.JSONObject
import java.util.Locale

class ShareIntentParser(
  private val contentResolver: ContentResolver,
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

    // The host must stage the URI while this activity still holds its read grant.
    return NativeSharePayload(kind = "png", uri = uri.toString(), name = displayName)
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
    private val HTTP_URL_REGEX = Regex("""https?://[^\s]+""", RegexOption.IGNORE_CASE)
  }
}

data class NativeSharePayload(
  val kind: String,
  val url: String? = null,
  val uri: String? = null,
  val name: String? = null,
) {
  fun toJsonObject(): JSONObject {
    return JSONObject().apply {
      put("kind", kind)
      url?.let { put("url", it) }
      uri?.let { put("uri", it) }
      name?.let { put("name", it) }
    }
  }
}
