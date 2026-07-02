package com.tauritavern.client

import android.content.ContentResolver
import android.net.Uri
import android.webkit.JavascriptInterface
import java.io.File

class AndroidImportArchiveJsBridge(
  private val contentResolver: ContentResolver,
  private val launchImportArchivePicker: () -> Unit,
  private val launchExportArchivePicker: (String) -> Unit,
) {
  @JavascriptInterface
  fun requestImportArchivePicker() {
    launchImportArchivePicker()
  }

  @JavascriptInterface
  fun requestExportArchivePicker(suggestedName: String?) {
    val normalizedName =
      suggestedName?.trim().orEmpty().ifBlank { DEFAULT_EXPORT_FILE_NAME }
    launchExportArchivePicker(normalizedName)
  }

  @JavascriptInterface
  fun stageContentUriToFile(
    contentUri: String?,
    targetPath: String?,
  ): String {
    val uri = Uri.parse(requireNotNull(contentUri).trim())
    val targetFile = File(requireNotNull(targetPath).trim())

    return AndroidContentFileTransfer.copyContentUriToFile(contentResolver, uri, targetFile)
  }

  @JavascriptInterface
  fun copyFileToContentUri(
    sourcePath: String?,
    contentUri: String?,
  ): String {
    val sourceFile = File(requireNotNull(sourcePath).trim())
    val targetUri = Uri.parse(requireNotNull(contentUri).trim())

    return AndroidContentFileTransfer.copyFileToContentUri(
      contentResolver = contentResolver,
      sourceFile = sourceFile,
      targetUri = targetUri,
      failureMessage = "Failed to copy export archive to destination URI",
    )
  }

  companion object {
    private const val DEFAULT_EXPORT_FILE_NAME = "tauritavern-data.zip"
    const val INTERFACE_NAME = "TauriTavernAndroidImportArchiveBridge"
  }
}
