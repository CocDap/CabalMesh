//! The Wi-Fi multicast lock that makes mDNS work on Android.
//!
//! # Why this exists
//!
//! Android drops multicast packets unless the app holds a
//! `WifiManager.MulticastLock`. It does not error and it does not prompt: mDNS
//! discovery returns zero peers while the swarm, the listeners and the relays
//! all look healthy. Declaring `CHANGE_WIFI_MULTICAST_STATE` in the manifest
//! only makes *acquiring* the lock legal — it does not enable multicast on its
//! own, which is the part that catches people out.
//!
//! # Not reachable from the webview
//!
//! Registered as a Tauri plugin so the JNI handle has somewhere to live, but no
//! command is exposed over IPC and the mobile capability file grants nothing
//! for it. Toggling radio state is not something the frontend has any reason to
//! ask for, and the grant would be permanent while the need is zero — the same
//! reasoning that keeps the keystore off the capability list.
//!
//! # The lock is tied to mesh activity, not app lifetime
//!
//! Holding it keeps the Wi-Fi radio in a higher-power state, which is a real
//! battery cost on a device that is idle in a pocket. It is acquired when the
//! mesh starts participating and released on suspend, alongside the offline
//! toggle that already tracks the same thing.

use tauri::{AppHandle, Manager, Runtime};

/// Whether local-network discovery can actually receive packets.
///
/// Three states rather than a `bool` because "no lock is needed here" and "the
/// OS refused" are different facts, and collapsing them would make one platform
/// claim a permission it never checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalNetwork {
    /// The lock is held. Multicast reaches the app.
    Granted,
    /// The OS refused the lock. Discovery will find nothing; the mesh still
    /// works over QUIC, TCP and relays.
    Denied,
    /// This platform needs no lock.
    ///
    /// Says nothing about whether discovery works. iOS gates local-network
    /// access behind a prompt with **no API to query the answer**, so its
    /// grant state stays unobserved rather than being assumed granted.
    NotApplicable,
}

impl LocalNetwork {
    /// Applies this observation to `mdns_granted`, leaving the flag untouched
    /// when the platform gave us nothing to observe.
    #[must_use]
    pub const fn apply_to(self, current: bool) -> bool {
        match self {
            Self::Granted => true,
            Self::Denied => false,
            Self::NotApplicable => current,
        }
    }
}

#[cfg(target_os = "android")]
mod android {
    use super::LocalNetwork;
    use serde::Deserialize;
    use tauri::{
        plugin::{Builder, PluginApi, PluginHandle, TauriPlugin},
        AppHandle, Manager, Runtime,
    };

    /// Matches the `JSObject` the Kotlin side resolves with.
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct AcquireResult {
        granted: bool,
    }

    /// The JNI handle, kept in managed state so both lifecycle edges can reach
    /// it without threading it through every call site.
    pub(super) struct Lock<R: Runtime>(PluginHandle<R>);

    pub(super) fn init<R: Runtime>() -> TauriPlugin<R> {
        Builder::new("multicast-lock")
            .setup(|app, api: PluginApi<R, ()>| {
                let handle = api.register_android_plugin("com.cabalmesh.app", "MulticastLockPlugin")?;
                app.manage(Lock(handle));
                Ok(())
            })
            .build()
    }

    pub(super) fn acquire<R: Runtime>(app: &AppHandle<R>) -> LocalNetwork {
        // A missing handle means plugin setup failed, which is a refusal from
        // the app's point of view: nothing holds the lock either way.
        let Some(lock) = app.try_state::<Lock<R>>() else {
            return LocalNetwork::Denied;
        };

        match lock.0.run_mobile_plugin::<AcquireResult>("acquire", ()) {
            Ok(result) if result.granted => LocalNetwork::Granted,
            Ok(_) => LocalNetwork::Denied,
            Err(error) => {
                tracing::warn!(target: "cabalmesh::multicast", %error, "could not acquire the multicast lock");
                LocalNetwork::Denied
            }
        }
    }

    pub(super) fn release<R: Runtime>(app: &AppHandle<R>) {
        let Some(lock) = app.try_state::<Lock<R>>() else {
            return;
        };
        if let Err(error) = lock.0.run_mobile_plugin::<()>("release", ()) {
            tracing::warn!(target: "cabalmesh::multicast", %error, "could not release the multicast lock");
        }
    }
}

/// Registers the plugin. A no-op outside Android.
#[must_use]
pub fn init<R: Runtime>() -> tauri::plugin::TauriPlugin<R> {
    #[cfg(target_os = "android")]
    {
        android::init()
    }
    #[cfg(not(target_os = "android"))]
    {
        tauri::plugin::Builder::new("multicast-lock").build()
    }
}

/// Takes the lock, reporting whether multicast can now be received.
///
/// Idempotent: acquiring while already held is a no-op that still reports
/// `Granted`, so a resume racing bootstrap cannot unbalance the reference
/// count.
pub fn acquire<R: Runtime>(app: &AppHandle<R>) -> LocalNetwork {
    #[cfg(target_os = "android")]
    {
        android::acquire(app)
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        LocalNetwork::NotApplicable
    }
}

/// Drops the lock, letting the Wi-Fi radio idle again.
pub fn release<R: Runtime>(app: &AppHandle<R>) {
    #[cfg(target_os = "android")]
    {
        android::release(app);
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
    }
}

/// Re-reads the grant and folds it into runtime capabilities.
///
/// Called on resume as well as at start: a user can revoke the permission from
/// Settings while the app is backgrounded, and an app that kept believing it
/// was granted would find no peers and blame the network.
pub fn refresh<R: Runtime>(app: &AppHandle<R>) {
    let Some(state) = app.try_state::<crate::state::AppState>() else {
        return;
    };
    let observed = acquire(app);
    let mut caps = state.runtime_caps();
    caps.mdns_granted = observed.apply_to(caps.mdns_granted);
    state.set_runtime_caps(caps);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_clears_the_flag() {
        assert!(!LocalNetwork::Denied.apply_to(true));
    }

    #[test]
    fn a_grant_sets_the_flag() {
        assert!(LocalNetwork::Granted.apply_to(false));
    }

    #[test]
    fn platforms_without_a_lock_observe_nothing() {
        // The point of the third variant: iOS must not be made to claim a
        // permission it has no way to query.
        assert!(!LocalNetwork::NotApplicable.apply_to(false));
        assert!(LocalNetwork::NotApplicable.apply_to(true));
    }
}
