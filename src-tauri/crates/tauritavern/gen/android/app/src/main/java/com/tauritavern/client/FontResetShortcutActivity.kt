package com.tauritavern.client

import android.app.Activity
import android.content.Intent
import android.os.Bundle

class FontResetShortcutActivity : Activity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    super.onCreate(savedInstanceState)
    if (intent.action == RESET_FONT_ACTION) {
      startActivity(Intent(this, MainActivity::class.java).setAction(RESET_FONT_ACTION))
    }
    finish()
  }

  companion object {
    const val RESET_FONT_ACTION = "com.tauritavern.client.RESET_DISPLAY_FONTS"
  }
}
