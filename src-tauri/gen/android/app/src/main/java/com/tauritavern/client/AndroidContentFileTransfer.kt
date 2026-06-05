package com.tauritavern.client

import android.content.ContentResolver
import android.net.Uri
import java.io.File
import java.io.FileInputStream
import java.io.FileOutputStream
import java.nio.channels.FileChannel

object AndroidContentFileTransfer {
  private const val COPY_BUFFER_BYTES = 4 * 1024 * 1024

  fun copyContentUriToFile(
    contentResolver: ContentResolver,
    contentUri: Uri,
    targetFile: File,
  ): String {
    targetFile.parentFile?.mkdirs()

    requireNotNull(contentResolver.openInputStream(contentUri)).use { input ->
      targetFile.outputStream().use { output -> input.copyTo(output, COPY_BUFFER_BYTES) }
    }

    return targetFile.absolutePath
  }

  fun copyFileToContentUri(
    contentResolver: ContentResolver,
    sourceFile: File,
    targetUri: Uri,
    failureMessage: String,
  ): String {
    FileInputStream(sourceFile).channel.use { source ->
      requireNotNull(contentResolver.openFileDescriptor(targetUri, "wt")).use { descriptor ->
        FileOutputStream(descriptor.fileDescriptor).channel.use { target ->
          transferFile(source, target, failureMessage)
          target.force(true)
        }
      }
    }

    return targetUri.toString()
  }

  private fun transferFile(
    source: FileChannel,
    target: FileChannel,
    failureMessage: String,
  ) {
    val size = source.size()
    var position = 0L

    while (position < size) {
      val transferred = source.transferTo(position, size - position, target)
      check(transferred > 0L) { failureMessage }
      position += transferred
    }
  }
}
