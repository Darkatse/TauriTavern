package com.tauritavern.client

import android.app.Activity
import android.net.Uri
import android.provider.OpenableColumns
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

@InvokeArg
class ContentUriArgs {
  lateinit var uri: String
}

@InvokeArg
class SharedUriArgs {
  lateinit var id: String
}

@TauriPlugin
class ContentUriPlugin(private val activity: Activity) : Plugin(activity) {
  @Command
  fun displayName(invoke: Invoke) {
    try {
      val uri = Uri.parse(invoke.parseArgs(ContentUriArgs::class.java).uri)
      val name = activity.contentResolver.query(
        uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null,
      )?.use { cursor ->
        check(cursor.moveToFirst()) { "Selected document has no metadata" }
        cursor.getString(cursor.getColumnIndexOrThrow(OpenableColumns.DISPLAY_NAME))
      }
      check(!name.isNullOrBlank()) { "Selected document has no display name" }
      invoke.resolve(JSObject().put("name", name))
    } catch (error: Exception) {
      invoke.reject("Failed to read selected document name: ${error.message}")
    }
  }

  @Command
  fun takeSharedUri(invoke: Invoke) {
    try {
      val id = invoke.parseArgs(SharedUriArgs::class.java).id
      val uri = checkNotNull((activity as MainActivity).sharedUris.take(id)) { "Unknown share" }
      invoke.resolve(JSObject().put("uri", uri.toString()))
    } catch (error: Exception) {
      invoke.reject("Failed to take shared file: ${error.message}")
    }
  }
}
