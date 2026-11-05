//! Handing rustls a trust store on Android.
//!
//! # The failure this prevents
//!
//! Every rustls feature of reqwest 0.13 pulls in `rustls-platform-verifier`
//! unconditionally — there is no webpki-roots alternative to select. On Android
//! that crate reads the system trust store through the JVM, and until it is
//! given a way in it panics on the **first HTTPS request**, not at startup:
//!
//! ```text
//! thread 'tokio-runtime-worker' panicked at rustls-platform-verifier/src/android.rs:90:10:
//! Expect rustls-platform-verifier to be initialized
//! ```
//!
//! Nothing about that is visible from a desktop or iOS build, which is what
//! makes it worth its own module: the app launches, the mesh comes up, and only
//! the balance fetch dies — on a background thread, where it reads as a missing
//! balance rather than a crash.
//!
//! # Why this is a JNI entry point rather than a call from `run()`
//!
//! The verifier needs an Android `Context`, and there is no way to ask for one
//! from Rust here. `ndk_context` — the usual answer — is **never populated in a
//! Tauri app**: tao's Android glue keeps the JVM and activity in its own
//! private state and does not call `initialize_android_context`, so
//! `ndk_context::android_context()` panics with "android context was not
//! initialized" before it can return anything to null-check.
//!
//! So the Context is pushed in from the Java side instead, at plugin load,
//! which is the first moment one exists. See `TlsPlugin.kt`.
//!
//! # Two halves, both required
//!
//! The verifier's own Kotlin component is added to the Gradle build (see
//! `gen/android/app/build.gradle.kts`); this is the Rust half. Either alone
//! still panics.

use tauri::Runtime;

/// Registers the plugin whose `load` performs the handshake. A no-op elsewhere.
///
/// Exposes no commands and needs no capability grant — the Kotlin side is
/// reached by Tauri's plugin loader, never over IPC.
#[must_use]
pub fn init<R: Runtime>() -> tauri::plugin::TauriPlugin<R> {
    let builder = tauri::plugin::Builder::new("cabalmesh-tls");

    #[cfg(target_os = "android")]
    let builder = builder.setup(|_app, api| {
        api.register_android_plugin("com.cabalmesh.app", "TlsPlugin")?;
        Ok(())
    });

    builder.build()
}

/// Initializes the platform trust store.
///
/// Called from Kotlin at plugin load, when a `Context` first exists. Failure is
/// logged rather than fatal: the mesh speaks noise over QUIC and TCP and needs
/// no certificate store, so an app that cannot reach the chain is still worth
/// launching — it just cannot show a balance.
///
/// # Safety
///
/// Called only by the JVM through the `TlsPlugin.initRustls` binding, with the
/// arguments JNI guarantees.
#[cfg(target_os = "android")]
#[no_mangle]
pub unsafe extern "system" fn Java_com_cabalmesh_app_TlsPlugin_initRustls(
    raw_env: *mut jni::sys::JNIEnv,
    _class: jni::sys::jclass,
    raw_context: jni::sys::jobject,
) {
    use jni::{errors::LogErrorAndDefault, objects::JObject, EnvUnowned};

    // SAFETY: the JVM passes a valid `JNIEnv` for the calling thread, which is
    // attached for the duration of the native call.
    let mut env = unsafe { EnvUnowned::from_raw(raw_env) };

    env.with_env(|env| {
        // SAFETY: a local reference owned by the calling JNI frame, which
        // outlives this closure. `JObject` is a non-owning wrapper in jni 0.22
        // — it has no `Drop` — so nothing here frees a reference it does not
        // own.
        let context = unsafe { JObject::from_raw(env, raw_context) };
        let result = rustls_platform_verifier::android::init_with_env(env, context);
        if result.is_ok() {
            tracing::debug!(target: "cabalmesh::tls", "platform trust store ready");
        }
        result
    })
    // Logged, not thrown: an exception here would take down the activity over
    // something the app can degrade around.
    .resolve::<LogErrorAndDefault>()
}
