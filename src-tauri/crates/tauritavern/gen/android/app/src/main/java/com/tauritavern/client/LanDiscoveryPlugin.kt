package com.tauritavern.client

import android.app.Activity
import android.net.wifi.WifiManager
import androidx.appcompat.app.AppCompatActivity
import androidx.lifecycle.Lifecycle
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.Plugin

@InvokeArg
class LanDiscoveryArgs {
  var enabled: Boolean = false
}

@TauriPlugin
class LanDiscoveryPlugin(private val activity: Activity) : Plugin(activity) {
  private val lifecycle = (activity as AppCompatActivity).lifecycle
  private var multicastLock: WifiManager.MulticastLock? = null

  @Command
  fun setEnabled(invoke: Invoke) {
    activity.runOnUiThread {
      try {
        val enabled = invoke.parseArgs(LanDiscoveryArgs::class.java).enabled
        if (enabled) {
          check(lifecycle.currentState.isAtLeast(Lifecycle.State.RESUMED)) {
            "LAN discovery requires the app to be in the foreground"
          }
          if (multicastLock == null) {
            val wifi = checkNotNull(activity.applicationContext.getSystemService(WifiManager::class.java)) {
              "Wi-Fi multicast service is unavailable"
            }
            val lock = wifi.createMulticastLock("TauriTavern LAN discovery")
            lock.setReferenceCounted(false)
            lock.acquire()
            multicastLock = lock
          }
        } else {
          releaseMulticastLock()
        }
        invoke.resolve()
      } catch (error: Exception) {
        invoke.reject("Failed to change LAN discovery multicast access: ${error.message}")
      }
    }
  }

  override fun onPause(activity: AppCompatActivity) {
    releaseMulticastLock()
  }

  override fun onDestroy(activity: AppCompatActivity) {
    releaseMulticastLock()
  }

  private fun releaseMulticastLock() {
    multicastLock?.let { if (it.isHeld) it.release() }
    multicastLock = null
  }
}
