package com.cabalmesh.keystore

import android.app.Activity
import android.util.Base64
import app.tauri.annotation.Command
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

/**
 * The Tauri boundary. Everything here is translation.
 *
 * The Keystore work is in [DeviceSecret]; this converts it into the JSON the
 * Rust side deserialises. Keeping the two apart means the Keystore code can be
 * read without knowing anything about Tauri.
 *
 * A failure is reported rather than substituted. Returning a made-up secret so
 * the call "succeeds" would produce a vault that opens today and refuses to
 * open after the next reinstall, which is worse than refusing now.
 */
@TauriPlugin
class KeystorePlugin(private val activity: Activity) : Plugin(activity) {
    @Command
    fun deviceSecret(invoke: Invoke) {
        try {
            val secret = DeviceSecret.get(activity.applicationContext)
            invoke.resolve(
                JSObject().apply {
                    put("secret", Base64.encodeToString(secret, Base64.NO_WRAP))
                    put("strongBox", DeviceSecret.strongBoxBacked)
                }
            )
        } catch (error: Throwable) {
            // The message reaches a log, never the webview: this plugin has no
            // webview surface at all.
            invoke.reject(error.message ?: "the Android Keystore refused")
        }
    }
}
