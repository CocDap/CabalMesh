package com.cabalmesh.app

import android.app.Activity
import android.content.Context
import android.net.wifi.WifiManager
import app.tauri.annotation.Command
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

/**
 * Holds a Wi-Fi multicast lock while the mesh is active.
 *
 * Android drops multicast packets unless a lock is held. It does not error and
 * it does not prompt: mDNS discovery simply returns zero peers while every
 * other part of the mesh looks healthy. The manifest permission alone is not
 * enough — CHANGE_WIFI_MULTICAST_STATE is what makes acquiring the lock legal,
 * not what enables multicast.
 *
 * The lock is tied to mesh activity rather than app lifetime because holding it
 * keeps the Wi-Fi radio in a higher-power state, which is a real battery cost.
 *
 * Invoked from Rust via `run_mobile_plugin`, never over IPC — see the plugin's
 * Rust side for why the webview gets no grant for this.
 */
@TauriPlugin
class MulticastLockPlugin(private val activity: Activity) : Plugin(activity) {
    private var lock: WifiManager.MulticastLock? = null

    @Command
    fun acquire(invoke: Invoke) {
        val result = JSObject()
        try {
            // Reference-counted so a resume that races an in-flight acquire
            // cannot leave the count negative and silently release the lock.
            val existing = lock
            if (existing != null && existing.isHeld) {
                result.put("granted", true)
                invoke.resolve(result)
                return
            }

            val wifi = activity.applicationContext
                .getSystemService(Context.WIFI_SERVICE) as WifiManager
            lock = wifi.createMulticastLock(LOCK_TAG).apply {
                setReferenceCounted(true)
                acquire()
            }
            result.put("granted", lock?.isHeld == true)
        } catch (e: SecurityException) {
            // The permission was revoked, or the OEM refuses the lock. This is
            // reported rather than thrown: the mesh still runs over QUIC/TCP
            // and relays, it just cannot discover peers on the local network,
            // and the nodes screen needs to say which of those it is.
            result.put("granted", false)
        }
        invoke.resolve(result)
    }

    @Command
    fun release(invoke: Invoke) {
        lock?.takeIf { it.isHeld }?.release()
        lock = null
        invoke.resolve()
    }

    private companion object {
        const val LOCK_TAG = "cabalmesh"
    }
}
