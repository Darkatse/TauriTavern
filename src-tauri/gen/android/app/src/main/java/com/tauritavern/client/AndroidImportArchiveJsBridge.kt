package com.tauritavern.client

import android.content.ContentResolver
import android.net.Uri
import android.webkit.JavascriptInterface
import java.io.File
import java.io.FileInputStream
import java.io.FileOutputStream
import java.nio.channels.FileChannel

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
    targetFile.parentFile?.mkdirs()

    requireNotNull(contentResolver.openInputStream(uri)).use { input ->
      targetFile.outputStream().use { output -> input.copyTo(output, COPY_BUFFER_BYTES) }
    }

    return targetFile.absolutePath
  }

  @JavascriptInterface
  fun copyFileToContentUri(
    sourcePath: String?,
    contentUri: String?,
  ): String {
    val sourceFile = File(requireNotNull(sourcePath).trim())
    val targetUri = Uri.parse(requireNotNull(contentUri).trim())

    FileInputStream(sourceFile).channel.use { source ->
      requireNotNull(contentResolver.openFileDescriptor(targetUri, "w")).use { descriptor ->
        FileOutputStream(descriptor.fileDescriptor).channel.use { target ->
          transferFile(source, target)
          target.force(true)
        }
      }
    }

    return targetUri.toString()
  }

  private fun transferFile(
    source: FileChannel,
    target: FileChannel,
  ) {
    val size = source.size()
    var position = 0L

    while (position < size) {
      val transferred = source.transferTo(position, size - position, target)
      check(transferred > 0L) { "Failed to copy export archive to destination URI" }
      position += transferred
    }
  }

  companion object {
    private const val COPY_BUFFER_BYTES = 4 * 1024 * 1024
    private const val DEFAULT_EXPORT_FILE_NAME = "tauritavern-data.zip"
    const val INTERFACE_NAME = "TauriTavernAndroidImportArchiveBridge"
  }
}
