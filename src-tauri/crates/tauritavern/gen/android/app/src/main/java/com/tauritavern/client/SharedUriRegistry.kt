package com.tauritavern.client

import android.net.Uri
import java.util.UUID
import java.util.concurrent.ConcurrentHashMap

/** Shared content URIs by id: the page names a share by id, and only the host reads its URI. */
class SharedUriRegistry {
  private val uris = ConcurrentHashMap<String, Uri>()

  fun add(uri: Uri): String = UUID.randomUUID().toString().also { uris[it] = uri }

  fun take(id: String): Uri? = uris.remove(id)
}
