package com.cabalmesh.ble

import android.app.Activity
import android.util.Base64
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Channel
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.util.UUID

@InvokeArg
class StartArgs {
    lateinit var serviceUuid: String
    lateinit var psmUuid: String
    lateinit var events: Channel
}

@InvokeArg
class SendArgs {
    var link: Long = 0
    lateinit var data: String
}

@InvokeArg
class LinkArgs {
    var link: Long = 0
}

/**
 * The Tauri boundary. Everything here is translation.
 *
 * The radio is in [BleRadio]; this converts its events into the JSON the Rust
 * side deserialises, and its arguments back. Keeping the two apart means the
 * BLE code can be read without knowing anything about Tauri, and the plugin
 * can be read without knowing anything about Bluetooth.
 */
@TauriPlugin
class BlePlugin(private val activity: Activity) : Plugin(activity) {
    private var radio: BleRadio? = null

    @Command
    fun start(invoke: Invoke) {
        val args = invoke.parseArgs(StartArgs::class.java)

        if (radio != null) {
            invoke.resolve(JSObject())
            return
        }

        val events = args.events
        val radio = BleRadio(
            activity.applicationContext,
            UUID.fromString(args.serviceUuid),
            UUID.fromString(args.psmUuid),
        ) { event -> events.send(encode(event)) }

        val failure = radio.start()
        if (failure != null) {
            // Resolved rather than rejected, and reported through the channel.
            //
            // A radio that will not run is a state the app must survive: the
            // IP plane is untouched and the nodes screen has to be able to say
            // "Bluetooth is off" rather than showing an error from a call that
            // was made correctly.
            events.send(encode(BleRadio.RadioEvent.Unavailable(failure)))
            invoke.resolve(JSObject())
            return
        }

        this.radio = radio
        invoke.resolve(JSObject())
    }

    @Command
    fun send(invoke: Invoke) {
        val args = invoke.parseArgs(SendArgs::class.java)
        val bytes = Base64.decode(args.data, Base64.NO_WRAP)
        radio?.send(args.link, bytes)
        // Resolved even when the link is gone: a peer walking out of range
        // between the decision to send and the write is ordinary, and the
        // engine learns about it from the link-down event rather than here.
        invoke.resolve(JSObject())
    }

    @Command
    fun close(invoke: Invoke) {
        val args = invoke.parseArgs(LinkArgs::class.java)
        radio?.tearDown(args.link)
        invoke.resolve(JSObject())
    }

    @Command
    fun stop(invoke: Invoke) {
        radio?.stop()
        radio = null
        invoke.resolve(JSObject())
    }

    /** One shape per event kind, tagged, matching the Rust enum. */
    private fun encode(event: BleRadio.RadioEvent): JSObject {
        val payload = JSObject()
        when (event) {
            is BleRadio.RadioEvent.Up -> {
                payload.put("kind", "up")
                payload.put("link", event.link)
            }
            is BleRadio.RadioEvent.Down -> {
                payload.put("kind", "down")
                payload.put("link", event.link)
            }
            is BleRadio.RadioEvent.Bytes -> {
                payload.put("kind", "bytes")
                payload.put("link", event.link)
                // NO_WRAP: the default inserts newlines, which the Rust decoder
                // refuses as characters outside the alphabet.
                payload.put("data", Base64.encodeToString(event.data, Base64.NO_WRAP))
            }
            is BleRadio.RadioEvent.Unavailable -> {
                payload.put("kind", "unavailable")
                payload.put("reason", event.reason)
            }
        }
        return payload
    }
}
