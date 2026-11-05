package com.cabalmesh.app

import android.app.Activity
import android.content.Context
import android.webkit.WebView
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Plugin

/**
 * Hands the Android `Context` to rustls before anything can make an HTTPS
 * request.
 *
 * `rustls-platform-verifier` reads the system trust store through the JVM and
 * panics on first use until initialized — see src/tls.rs for the shape of that
 * failure and why the Context has to arrive from this side rather than being
 * fetched from Rust.
 *
 * This plugin exposes no commands. It exists for `load`, which is the earliest
 * point at which an Activity exists, and is still well before bootstrap reaches
 * the RPC endpoint.
 */
@TauriPlugin
class TlsPlugin(private val activity: Activity) : Plugin(activity) {
    override fun load(webView: WebView) {
        // The application context, not the activity: the verifier holds a
        // global reference to whatever it is given, and holding the activity
        // would leak it across rotation.
        initRustls(activity.applicationContext)
    }

    /**
     * Implemented in Rust as `Java_com_cabalmesh_app_TlsPlugin_initRustls`.
     *
     * No `System.loadLibrary` here — Tauri has already loaded `cabalmesh_lib`
     * by the time a plugin loads, and loading it twice from a second
     * ClassLoader is how you get `UnsatisfiedLinkError` on release builds.
     */
    private external fun initRustls(context: Context)
}
