//! Suspend and resume handling.
//!
//! # No custom plugin needed
//!
//! An earlier plan specified a native lifecycle plugin. That was written
//! against Tauri 2.9, where nothing propagated. **2.11 emits
//! `WindowEvent::Suspended` and `WindowEvent::Resumed` on mobile**, so the
//! plugin is obsolete before it was written.
//!
//! From the Tauri source, the mapping is:
//!
//! | | Suspended | Resumed |
//! |---|---|---|
//! | Android | `Activity.onPause` | `Activity.onResume` (first one ignored) |
//! | iOS | `applicationWillResignActive` | `applicationWillEnterForeground` |
//! | Desktop | not emitted | not emitted |
//!
//! # The iOS asymmetry, and why teardown is conservative
//!
//! Those two iOS callbacks are **not** mirror images. `willResignActive` fires
//! for transient interruptions — the control centre, the notification shade, an
//! incoming call, a permission prompt — while `willEnterForeground` only fires
//! after actually leaving the background.
//!
//! So a pull-down of the notification shade delivers `Suspended` with no
//! matching `Resumed` until the app is genuinely backgrounded and returned to.
//! Anything torn down on suspend must therefore be cheap to lose and
//! reconstructed on demand, not only on resume — otherwise glancing at a
//! notification silently kills the mesh for the rest of the session.
//!
//! What this does on suspend is bounded accordingly: cancel live streams and
//! stop mesh participation. Both are re-established by the next subscribe or
//! publish, so the transient case self-heals. The swarm itself is deliberately
//! left running — see `MeshHandle::set_offline`.

use crate::state::AppState;
use tauri::{AppHandle, Manager, Runtime};

/// Handles the app being suspended.
///
/// Streams are cancelled because a backgrounded webview cannot receive them, so
/// producing into it is pure battery cost. Mesh participation stops, which also
/// honours the offline promise while the OS has us paused.
pub fn on_suspend<R: Runtime>(app: &AppHandle<R>) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };

    let live = state.subscriptions().len();
    state.subscriptions().cancel_all();

    tracing::info!(
        target: "cabalmesh::lifecycle",
        cancelled_streams = live,
        "suspended"
    );

    if let Ok(services) = state.services() {
        if let Some(mesh) = services.mesh.clone() {
            tauri::async_runtime::spawn(async move {
                if let Err(error) = mesh.set_offline(true).await {
                    tracing::warn!(target: "cabalmesh::lifecycle", %error, "could not pause the mesh");
                }
            });
        }
    }

    // Released rather than held across the background, because the lock keeps
    // the Wi-Fi radio awake and a paused app has nothing to discover with it.
    crate::multicast::release(app);
}

/// Handles the app returning to the foreground.
///
/// Runtime capabilities are re-read rather than assumed: a user can revoke
/// Local Network access from Settings while the app is backgrounded, and an app
/// that kept believing it was granted would silently find no peers and blame
/// the network.
pub fn on_resume<R: Runtime>(app: &AppHandle<R>) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };

    tracing::info!(target: "cabalmesh::lifecycle", "resumed");

    // Re-read, not restored from what we believed on suspend: the permission
    // can be revoked from Settings while the app is backgrounded.
    crate::multicast::refresh(app);

    if let Ok(services) = state.services() {
        if let Some(mesh) = services.mesh.clone() {
            tauri::async_runtime::spawn(async move {
                if let Err(error) = mesh.set_offline(false).await {
                    tracing::warn!(target: "cabalmesh::lifecycle", %error, "could not resume the mesh");
                }
            });
        }
    }

    // Streams are not restarted here. The frontend re-subscribes when its
    // screens remount, and guessing which streams a resumed UI wants would
    // recreate ones nothing is listening to.
}

#[cfg(test)]
mod tests {
    use crate::state::AppState;

    #[test]
    fn suspending_cancels_live_streams() {
        // Exercises the state effect directly; AppHandle cannot be constructed
        // outside a Tauri app, and the handler is a thin wrapper over this.
        let state = AppState::new();
        let (_id, token) = state.subscriptions().register("mesh-log").unwrap();

        state.subscriptions().cancel_all();

        assert!(token.is_cancelled());
        assert!(state.subscriptions().is_empty());
    }

    #[test]
    fn suspending_before_bootstrap_is_harmless() {
        // Suspension can arrive during launch, before services exist. It must
        // not panic on the missing subsystem.
        let state = AppState::new();
        assert!(state.services().is_err());
        state.subscriptions().cancel_all();
    }
}
