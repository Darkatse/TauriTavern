package com.tauritavern.client

import android.app.Activity
import android.net.wifi.WifiManager
import androidx.appcompat.app.AppCompatActivity
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.ProcessLifecycleOwner
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
class LanDiscoveryPlugin(private val activity: Activity) : Plugin(activity), DefaultLifecycleObserver {
  // Match Wry's lifecycle source so every release has a corresponding Rust resume event.
  private val lifecycle = ProcessLifecycleOwner.get().lifecycle
  private var multicastLock: WifiManager.MulticastLock? = null

  init {
    activity.runOnUiThread { lifecycle.addObserver(this) }
  }

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

  override fun onPause(owner: LifecycleOwner) {
    releaseMulticastLock()
  }

  override fun onDestroy(activity: AppCompatActivity) {
    lifecycle.removeObserver(this)
    releaseMulticastLock()
  }

  private fun releaseMulticastLock() {
    multicastLock?.let { if (it.isHeld) it.release() }
    multicastLock = null
  }
}
