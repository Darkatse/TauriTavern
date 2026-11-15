package com.tauritavern.client

import android.webkit.WebResourceResponse
import java.io.IOException
import java.io.InputStream

internal object HostResourceBodyStream {
    private const val HEADER = "x-tauritavern-body-stream"

    fun attach(response: WebResourceResponse?) {
        val value = response?.responseHeaders?.remove(HEADER) ?: return
        val (id, length) = value.split(' ').map(String::toLong)
        response.data = Body(id, length)
    }

    @JvmStatic external fun read(id: Long, buffer: ByteArray, offset: Int, length: Int): Int
    @JvmStatic external fun close(id: Long)

    private class Body(private val id: Long, private var remaining: Long) : InputStream() {
        override fun read(): Int {
            val byte = ByteArray(1)
            return if (read(byte, 0, 1) == -1) -1 else byte[0].toInt() and 0xff
        }

        override fun read(buffer: ByteArray, offset: Int, length: Int): Int {
            if (remaining == 0L) return -1
            if (length == 0) return 0
            val count = HostResourceBodyStream.read(id, buffer, offset, minOf(length.toLong(), remaining).toInt())
            if (count <= 0) throw IOException("Host resource ended $remaining bytes early")
            remaining -= count
            return count
        }

        override fun available(): Int = minOf(remaining, Int.MAX_VALUE.toLong()).toInt()

        override fun close() = HostResourceBodyStream.close(id)

        // WebView does not close responses discarded before opening.
        protected fun finalize() = close()
    }
}
