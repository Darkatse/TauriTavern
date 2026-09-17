package com.tauritavern.client

import android.app.Activity
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.speech.tts.TextToSpeech
import android.speech.tts.UtteranceProgressListener
import androidx.appcompat.app.AppCompatActivity
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Channel
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import org.json.JSONArray
import java.util.Locale

@InvokeArg
class SpeechInitializeArgs {
  lateinit var events: Channel
}

@InvokeArg
class SpeechUtteranceArgs {
  lateinit var id: String
  lateinit var text: String
  var lang: String = ""
  var voice: String? = null
  var rate: Float = 1f
  var pitch: Float = 1f
  var volume: Float = 1f
}

@TauriPlugin
class SpeechSynthesisPlugin(private val activity: Activity) : Plugin(activity) {
  private val main = Handler(Looper.getMainLooper())
  private var engine: TextToSpeech? = null
  private var ready = false
  private var events: Channel? = null
  private val initializations = mutableListOf<Invoke>()

  @Command
  fun initialize(invoke: Invoke) {
    main.post {
      try {
        events = invoke.parseArgs(SpeechInitializeArgs::class.java).events
        if (ready) {
          // A new document replaces the previous document's speech connection.
          engine!!.stop()
          invoke.resolve(voiceList())
        } else {
          initializations.add(invoke)
          if (engine == null) {
            engine = TextToSpeech(activity.applicationContext) { status ->
              // OnInit may run before the constructor returns.
              main.post { finishInitialization(status) }
            }
          }
        }
      } catch (error: Exception) {
        val waiting = initializations.contains(invoke)
        failInitialization(error.message ?: "Failed to initialize system TTS")
        if (!waiting) invoke.reject(error.message, "synthesis-unavailable")
      }
    }
  }

  private fun finishInitialization(status: Int) {
    if (status != TextToSpeech.SUCCESS) {
      failInitialization("Android could not initialize the system TTS engine")
      return
    }
    try {
      engine!!.setOnUtteranceProgressListener(object : UtteranceProgressListener() {
        override fun onStart(id: String) = emit(id, "start")
        override fun onDone(id: String) = emit(id, "end")
        override fun onError(id: String) = emit(id, "error", "synthesis-failed")
        override fun onError(id: String, code: Int) = emit(id, "error", when (code) {
          TextToSpeech.ERROR_NETWORK, TextToSpeech.ERROR_NETWORK_TIMEOUT -> "network"
          TextToSpeech.ERROR_NOT_INSTALLED_YET -> "voice-unavailable"
          TextToSpeech.ERROR_INVALID_REQUEST -> "invalid-argument"
          TextToSpeech.ERROR_OUTPUT -> "audio-hardware"
          else -> "synthesis-failed"
        })
        override fun onStop(id: String, interrupted: Boolean) =
          emit(id, "error", if (interrupted) "interrupted" else "canceled")
      })
      val voices = voiceList()
      ready = true
      initializations.forEach { it.resolve(voices) }
      initializations.clear()
    } catch (error: Exception) {
      failInitialization(error.message ?: "Failed to initialize system TTS")
    }
  }

  private fun failInitialization(message: String) {
    engine?.shutdown()
    engine = null
    ready = false
    initializations.forEach { it.reject(message, "synthesis-unavailable") }
    initializations.clear()
  }

  private fun voiceList(): JSObject {
    val tts = engine!!
    val defaultVoice = tts.defaultVoice
    val voices = tts.voices.orEmpty().sortedBy { it.name }.map { voice ->
      JSObject()
        .put("voiceURI", voice.name)
        .put("name", voice.name)
        .put("lang", voice.locale.toLanguageTag())
        .put("localService", !voice.isNetworkConnectionRequired)
        .put("default", voice == defaultVoice)
    }
    return JSObject().put("voices", JSONArray(voices))
  }

  @Command
  fun speak(invoke: Invoke) {
    main.post {
      try {
        val args = invoke.parseArgs(SpeechUtteranceArgs::class.java)
        val tts = engine
        if (!ready || tts == null) {
          invoke.reject("System TTS is not initialized", "synthesis-unavailable")
          return@post
        }
        if (args.text.length > TextToSpeech.getMaxSpeechInputLength()) {
          invoke.reject("Text exceeds the Android TTS input limit", "text-too-long")
          return@post
        }
        if (!args.rate.isFinite() || args.rate <= 0 || !args.pitch.isFinite() || args.pitch < 0 ||
          !args.volume.isFinite() || args.volume !in 0f..1f) {
          invoke.reject("Invalid speech rate, pitch or volume", "invalid-argument")
          return@post
        }
        if (args.voice != null) {
          val voice = tts.voices.orEmpty().find { it.name == args.voice }
          if (voice == null || tts.setVoice(voice) != TextToSpeech.SUCCESS) {
            invoke.reject("The requested system voice is unavailable", "voice-unavailable")
            return@post
          }
        } else {
          val locale = if (args.lang.isEmpty()) Locale.getDefault() else Locale.forLanguageTag(args.lang)
          if (tts.setLanguage(locale) < TextToSpeech.LANG_AVAILABLE) {
            invoke.reject("No installed system voice supports ${locale.toLanguageTag()}", "language-unavailable")
            return@post
          }
        }
        // Web Speech allows zero pitch; Android accepts positive hundredths only.
        if (tts.setSpeechRate(args.rate) != TextToSpeech.SUCCESS ||
          tts.setPitch(args.pitch.coerceAtLeast(0.01f)) != TextToSpeech.SUCCESS) {
          invoke.reject("The TTS engine rejected the speech parameters", "invalid-argument")
          return@post
        }
        val params = Bundle().apply { putFloat(TextToSpeech.Engine.KEY_PARAM_VOLUME, args.volume) }
        if (tts.speak(args.text, TextToSpeech.QUEUE_ADD, params, args.id) != TextToSpeech.SUCCESS) {
          invoke.reject("The TTS engine rejected the utterance", "synthesis-failed")
        } else {
          invoke.resolve()
        }
      } catch (error: Exception) {
        invoke.reject(error.message, "synthesis-failed")
      }
    }
  }

  @Command
  fun cancel(invoke: Invoke) {
    main.post {
      try {
        if (engine?.stop() == TextToSpeech.ERROR) invoke.reject("Failed to stop system TTS")
        else invoke.resolve()
      } catch (error: Exception) {
        invoke.reject(error.message)
      }
    }
  }

  private fun emit(id: String, type: String, error: String? = null) {
    main.post {
      events?.send(JSObject().put("id", id).put("type", type).apply {
        if (error != null) put("error", error)
      })
    }
  }

  override fun onDestroy(activity: AppCompatActivity) {
    engine?.stop()
    failInitialization("System TTS was closed")
    events = null
  }
}
