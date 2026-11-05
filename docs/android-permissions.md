# Android network permissions — applied

Prepared by ticket 21 and **applied on 2026-08-03**, once ticket 08 ran
`tauri android init` and there was a project tree to write into.

What shipped differs from the sketch below in two ways worth knowing:

- The lock is **reference-counted and idempotent** — acquiring while already
  held reports `Granted` and takes nothing further, so a resume that races
  bootstrap cannot unbalance the count and silently drop the lock.
- A refused lock is **reported, not thrown**. A `SecurityException` from a
  revoked permission or an unusual OEM leaves the mesh running over QUIC, TCP
  and relays; it just cannot discover peers locally, and the nodes screen needs
  to be able to say which of those it is.

The Rust side lives in `src-tauri/src/multicast.rs`, the Kotlin in
`gen/android/app/src/main/java/com/cabalmesh/app/MulticastLockPlugin.kt`.

## Why this is easy to get wrong

Android **silently drops multicast packets** unless the app holds a
`WifiManager.MulticastLock`. It does not error and it does not prompt —
discovery simply returns zero peers while every other part of the mesh looks
healthy. It is the same failure shape as the iOS local-network keys, and just as
unpleasant to debug on a device.

So a working Android mesh needs **both** the manifest permissions and the
runtime lock. Either one alone leaves discovery dead.

## 1. Manifest

`src-tauri/gen/android/app/src/main/AndroidManifest.xml`, inside `<manifest>`
and before `<application>`:

```xml
<uses-permission android:name="android.permission.INTERNET" />
<uses-permission android:name="android.permission.ACCESS_NETWORK_STATE" />

<!-- Required for mDNS. Without CHANGE_WIFI_MULTICAST_STATE the multicast
     lock below cannot be acquired, and multicast is dropped silently. -->
<uses-permission android:name="android.permission.CHANGE_WIFI_MULTICAST_STATE" />
<uses-permission android:name="android.permission.ACCESS_WIFI_STATE" />
```

That tree **is** version-controlled — the repo ignores only
`src-tauri/gen/schemas` — so edits persist across ordinary builds. Re-running
`tauri android init` is what to be careful with, not `tauri android build`.

## 2. Multicast lock plugin

A Tauri mobile plugin holding the lock while the mesh is active:

```kotlin
@TauriPlugin
class MulticastLockPlugin(private val activity: Activity) : Plugin(activity) {
    private var lock: WifiManager.MulticastLock? = null

    @Command
    fun acquire(invoke: Invoke) {
        val wifi = activity.applicationContext
            .getSystemService(Context.WIFI_SERVICE) as WifiManager
        lock = wifi.createMulticastLock("cabalmesh").apply {
            setReferenceCounted(true)
            acquire()
        }
        invoke.resolve()
    }

    @Command
    fun release(invoke: Invoke) {
        lock?.takeIf { it.isHeld }?.release()
        lock = null
        invoke.resolve()
    }
}
```

Held while the mesh runs, released on pause. A permanently held lock keeps the
Wi-Fi radio in a higher-power state and is a real battery cost, so it is tied to
mesh activity rather than to app lifetime.

**No webview capability grant.** This is invoked from Rust via
`run_mobile_plugin`, never over IPC. Granting the frontend the ability to toggle
radio state would be an over-grant with no caller — the same reasoning that
keeps `keystore` off the mobile capability list.

## 3. Feeding it back to the UI

The grant result belongs in `RuntimeCaps.mdns_granted`, re-read on resume
because a user can revoke permissions while the app is backgrounded. The nodes
screen then distinguishes "no peers nearby" from "no way to look for peers",
which are different messages.

## Verification that actually proves something

A build succeeding proves nothing here — the failure is silent at runtime. The
check is two physical Android devices on one Wi-Fi network discovering each
other, and the same test with the permission denied showing the explanatory
empty state rather than an indefinite spinner.
