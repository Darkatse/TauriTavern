package com.tauritavern.client

import android.content.ContentResolver
import android.content.ContentValues
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.MediaStore
import android.webkit.JavascriptInterface
import org.json.JSONObject
import java.io.File
import java.util.Locale

class AndroidPublicDownloadJsBridge(
  private val contentResolver: ContentResolver,
  private val exportStagingRoot: File,
  private val launchCreateDocumentPicker: (String, String) -> Unit,
) {
  @JavascriptInterface
  fun supportsDirectPublicDownloads(): Boolean = Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q

  @JavascriptInterface
  fun requestCreateDocumentPicker(
    suggestedName: String?,
    mimeType: String?,
  ) {
    launchCreateDocumentPicker(
      normalizeDisplayName(suggestedName),
      normalizeMimeType(mimeType),
    )
  }

  @JavascriptInterface
  fun copyFileToContentUri(
    sourcePath: String?,
    contentUri: String?,
  ): String {
    val sourceFile = resolveSourceFile(sourcePath)
    val targetUri = Uri.parse(requireNotNull(contentUri).trim())

    return AndroidContentFileTransfer.copyFileToContentUri(
      contentResolver = contentResolver,
      sourceFile = sourceFile,
      targetUri = targetUri,
      failureMessage = "Failed to copy export file to destination URI",
    )
  }

  @JavascriptInterface
  fun saveFileToDownloads(
    sourcePath: String?,
    displayName: String?,
    mimeType: String?,
  ): String {
    require(supportsDirectPublicDownloads()) {
      "Direct public Downloads export requires Android 10 or newer"
    }

    val sourceFile = resolveSourceFile(sourcePath)
    val normalizedName = normalizeDisplayName(displayName)
    val normalizedMimeType = normalizeMimeType(mimeType)
    val values =
      ContentValues().apply {
        put(MediaStore.MediaColumns.DISPLAY_NAME, normalizedName)
        put(MediaStore.MediaColumns.MIME_TYPE, normalizedMimeType)
        put(MediaStore.MediaColumns.RELATIVE_PATH, Environment.DIRECTORY_DOWNLOADS)
        put(MediaStore.MediaColumns.IS_PENDING, 1)
        put(MediaStore.MediaColumns.SIZE, sourceFile.length())
      }

    val uri =
      requireNotNull(contentResolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values)) {
        "Failed to create public Downloads entry"
      }
    var committed = false

    try {
      AndroidContentFileTransfer.copyFileToContentUri(
        contentResolver = contentResolver,
        sourceFile = sourceFile,
        targetUri = uri,
        failureMessage = "Failed to copy export file to public Downloads",
      )

      val publishValues =
        ContentValues().apply {
          put(MediaStore.MediaColumns.IS_PENDING, 0)
        }
      val updated = contentResolver.update(uri, publishValues, null, null)
      check(updated > 0) { "Failed to publish public Downloads entry" }
      committed = true

      return JSONObject()
        .apply {
          put("uri", uri.toString())
          put("display_name", normalizedName)
          put("mime_type", normalizedMimeType)
          put("relative_path", "${Environment.DIRECTORY_DOWNLOADS}/$normalizedName")
          put("saved_path", publicDownloadDisplayPath(normalizedName))
        }.toString()
    } finally {
      if (!committed) {
        contentResolver.delete(uri, null, null)
      }
    }
  }

  private fun resolveSourceFile(sourcePath: String?): File {
    val sourceFile = File(requireNotNull(sourcePath).trim()).canonicalFile
    val stagingRoot = exportStagingRoot.canonicalFile
    require(sourceFile.isFile) { "Export staging file not found: ${sourceFile.absolutePath}" }
    require(sourceFile.isDescendantOf(stagingRoot)) {
      "Export source file is outside the public download staging directory"
    }
    return sourceFile
  }

  private fun normalizeDisplayName(value: String?): String {
    val normalizedName = value?.trim().orEmpty().ifBlank { DEFAULT_DOWNLOAD_FILE_NAME }
    require(!normalizedName.contains('/') && !normalizedName.contains('\\')) {
      "Export filename must not contain path separators"
    }
    return normalizedName
  }

  private fun normalizeMimeType(value: String?): String {
    val mimeType = value
      ?.substringBefore(';')
      ?.trim()
      .orEmpty()

    return if (MIME_TYPE_PATTERN.matches(mimeType)) {
      mimeType.lowercase(Locale.ROOT)
    } else {
      DEFAULT_MIME_TYPE
    }
  }

  private fun File.isDescendantOf(directory: File): Boolean {
    val directoryPath = directory.path
    return path.startsWith("$directoryPath${File.separator}")
  }

  @Suppress("DEPRECATION")
  private fun publicDownloadDisplayPath(displayName: String): String =
    File(Environment.getExternalStorageDirectory(), "${Environment.DIRECTORY_DOWNLOADS}/$displayName")
      .absolutePath

  companion object {
    private const val DEFAULT_DOWNLOAD_FILE_NAME = "download.bin"
    private const val DEFAULT_MIME_TYPE = "application/octet-stream"
    private val MIME_TYPE_PATTERN =
      Regex("^[A-Za-z0-9][A-Za-z0-9!#$&^_.+-]*/[A-Za-z0-9][A-Za-z0-9!#$&^_.+-]*$")
    const val EXPORT_STAGING_ROOT_NAME = "tauritavern-export-staging"
    const val INTERFACE_NAME = "TauriTavernAndroidPublicDownloadBridge"
  }
}
