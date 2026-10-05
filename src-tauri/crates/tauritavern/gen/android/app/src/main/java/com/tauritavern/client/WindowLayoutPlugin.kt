package com.tauritavern.client

import android.app.Activity
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Color
import android.util.Base64
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import org.json.JSONObject

@InvokeArg
class BackdropBeginArgs {
  var windowRevision: Long = 0
  var replaceWallpaper: Boolean = false
  lateinit var color: IntArray
}

@InvokeArg
class BackdropStripArgs {
  var x: Int = 0
  var y: Int = 0
  lateinit var png: String
}

@InvokeArg
class BackdropApplyArgs {
  var windowRevision: Long = 0
  var wallpaperToken: Long = 0
  lateinit var strips: Array<BackdropStripArgs>
}

@TauriPlugin
class WindowLayoutPlugin(private val activity: Activity) : Plugin(activity) {
  private val layout get(): AndroidWindowLayout {
    // Tauri's plugin manager retains its first Activity across Activity recreation.
    check(!activity.isDestroyed) { "Activity was recreated; restart the app to restore window layout" }
    return (activity as MainActivity).windowLayout
  }

  private fun onUi(invoke: Invoke, action: () -> JSObject) {
    activity.runOnUiThread {
      try { invoke.resolve(action()) }
      catch (error: Exception) { invoke.reject("Window layout: ${error.message}") }
    }
  }

  @Command
  fun snapshot(invoke: Invoke) = onUi(invoke) { JSObject(layout.getSnapshot().toString()) }

  @Command
  fun beginBackdrop(invoke: Invoke) {
    val args = invoke.parseArgs(BackdropBeginArgs::class.java)
    onUi(invoke) {
      JSObject().put("target", layout.beginBackdrop(args.windowRevision,
        Color.rgb(args.color[0], args.color[1], args.color[2]), args.replaceWallpaper) ?: JSONObject.NULL)
    }
  }

  @Command
  fun applyBackdrop(invoke: Invoke) {
    try {
      val args = invoke.parseArgs(BackdropApplyArgs::class.java)
      val strips = args.strips.map {
        val bytes = Base64.decode(it.png, Base64.NO_WRAP)
        val bitmap = checkNotNull(BitmapFactory.decodeByteArray(bytes, 0, bytes.size)) {
          "Invalid backdrop PNG"
        }
        // Rust already rendered physical window pixels; Canvas must not rescale them.
        bitmap.density = Bitmap.DENSITY_NONE
        WindowBackdropStrip(it.x, it.y, bitmap)
      }
      onUi(invoke) {
        layout.applyBackdrop(args.windowRevision, args.wallpaperToken, strips)
        JSObject()
      }
    } catch (error: Exception) {
      invoke.reject("Window backdrop: ${error.message}")
    }
  }
}
