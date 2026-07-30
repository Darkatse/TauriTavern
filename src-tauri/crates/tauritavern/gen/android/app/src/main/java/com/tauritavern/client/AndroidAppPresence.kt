package com.tauritavern.client

object AndroidAppPresence {
  @Volatile
  private var activityResumed: Boolean = false

  @Volatile
  private var windowFocused: Boolean = false

  fun setActivityResumed(value: Boolean): Boolean {
    val wasForegroundInteractive = isForegroundInteractive()
    activityResumed = value
    if (!value) {
      windowFocused = false
    }
    return !wasForegroundInteractive && isForegroundInteractive()
  }

  fun setWindowFocused(value: Boolean): Boolean {
    val wasForegroundInteractive = isForegroundInteractive()
    windowFocused = value
    return !wasForegroundInteractive && isForegroundInteractive()
  }

  fun isForegroundInteractive(): Boolean = activityResumed && windowFocused
}
