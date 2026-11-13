//! A device secret released **only after the person proves presence with
//! the device passcode** — what makes a 6-digit PIN defensible.
//!
//! # Why this, and not "a hardware retry counter for the PIN"
//!
//! Neither the Secure Enclave nor StrongBox counts attempts at a secret an
//! *app* defines. What they count, in hardware, is the *device* passcode.
//! So the vault key under a PIN is `H(Argon2id(PIN) ‖ presence secret)`,
//! and the presence secret only leaves the key store after the OS has
//! checked the device passcode, under the OS's own hardware-enforced
//! throttling. Consequences, stated plainly:
//!
//! - A copied vault file, or a stolen locked phone: the PIN alone is
//!   useless, and getting the presence secret means beating the device
//!   passcode's hardware limit. This is the case the doc's decision 1 is
//!   about.
//! - Someone who already knows the device passcode and holds the phone can
//!   guess the 6-digit PIN at Argon2id speed, limited only by the software
//!   backoff in `unlock_attempts`. Against that person the PIN is a speed
//!   bump, exactly as a passphrase-less phone would be.
//!
//! Device passcode only, never biometrics: the doc treats biometrics as the
//! most identifying input there is and keeps them optional.
//!
//! # Where it exists
//!
//! | Platform | Store | Available |
//! |---|---|---|
//! | iOS | Keychain item, `kSecAccessControlDevicePasscode` | yes |
//! | Android 11+ (API 30) | Keystore key, `AUTH_DEVICE_CREDENTIAL`, via `tauri-plugin-cabal-keystore` | if the plugin reports it |
//! | Desktop, Android < 11 | — | no: passphrase only |
//!
//! **Not verified on a physical device** in this change — compiled and unit
//! tested on the host only. See `docs/identity-design.md`.

use cabal_vault::VaultError;
use zeroize::Zeroizing;

/// Envelope name recorded for a vault peppered with this secret.
pub const PEPPER_NAME: &str = "device-presence";

type Source = Box<dyn Fn() -> Result<Vec<u8>, String> + Send + Sync>;
static ANDROID_SOURCE: std::sync::OnceLock<Source> = std::sync::OnceLock::new();

/// Installs the Android presence source, once, at startup — only when the
/// plugin reported the platform supports it.
pub fn install_android_source(source: Source) {
    let _ = ANDROID_SOURCE.set(source);
}

#[cfg(test)]
thread_local! {
    static TEST_SECRET: std::cell::RefCell<Option<Vec<u8>>> = const { std::cell::RefCell::new(None) };
}

/// Test-only: pretend this platform has a presence store holding `secret`.
#[cfg(test)]
pub(crate) fn with_test_secret<T>(secret: Option<Vec<u8>>, body: impl FnOnce() -> T) -> T {
    TEST_SECRET.with(|s| *s.borrow_mut() = secret);
    let out = body();
    TEST_SECRET.with(|s| *s.borrow_mut() = None);
    out
}

/// Whether a PIN may be offered on this device.
#[must_use]
pub fn available() -> bool {
    #[cfg(test)]
    {
        return TEST_SECRET.with(|s| s.borrow().is_some());
    }
    #[cfg(not(test))]
    {
        cfg!(target_os = "ios") || (cfg!(target_os = "android") && ANDROID_SOURCE.get().is_some())
    }
}

/// The presence secret, creating it on first use. On a real device this
/// shows the system passcode prompt and blocks until it is answered, so
/// call it off the async runtime.
///
/// # Errors
///
/// [`VaultError::KeyUnavailable`] if the platform has no presence store, the
/// person cancelled, or the store refused.
pub fn secret() -> Result<Zeroizing<Vec<u8>>, VaultError> {
    #[cfg(test)]
    {
        return TEST_SECRET.with(|s| s.borrow().clone()).map(Zeroizing::new).ok_or(VaultError::KeyUnavailable);
    }
    #[cfg(not(test))]
    {
        platform_secret()
    }
}

#[cfg(all(not(test), target_os = "ios"))]
fn platform_secret() -> Result<Zeroizing<Vec<u8>>, VaultError> {
    use security_framework::passwords::{generic_password, set_generic_password_options};
    use security_framework::passwords_options::{AccessControlOptions, PasswordOptions};

    const SERVICE: &str = "com.cabalmesh.vault";
    const ACCOUNT: &str = "vault-presence-secret";
    let options = || {
        let mut options = PasswordOptions::new_generic_password(SERVICE, ACCOUNT);
        options.set_access_control_options(AccessControlOptions::DEVICE_PASSCODE);
        options
    };

    match generic_password(options()) {
        Ok(secret) if secret.len() == 32 => Ok(Zeroizing::new(secret)),
        Ok(_) => Err(VaultError::KeyUnavailable),
        // errSecItemNotFound: first use. Any other error (cancelled prompt,
        // no passcode set) is a refusal, never a reason to make a new one —
        // that would orphan the vault.
        Err(error) if error.code() == -25300 => {
            let mut fresh = Zeroizing::new(vec![0_u8; 32]);
            aes_gcm::aead::rand_core::RngCore::fill_bytes(&mut aes_gcm::aead::OsRng, &mut fresh);
            set_generic_password_options(&fresh, options()).map_err(|_| VaultError::KeyUnavailable)?;
            Ok(fresh)
        }
        Err(_) => Err(VaultError::KeyUnavailable),
    }
}

#[cfg(all(not(test), not(target_os = "ios")))]
fn platform_secret() -> Result<Zeroizing<Vec<u8>>, VaultError> {
    let source = ANDROID_SOURCE.get().ok_or(VaultError::KeyUnavailable)?;
    match source() {
        Ok(secret) if secret.len() == 32 => Ok(Zeroizing::new(secret)),
        Ok(_) => Err(VaultError::KeyUnavailable),
        Err(error) => {
            tracing::warn!(target: "cabalmesh::vault", %error, "the presence secret was refused");
            Err(VaultError::KeyUnavailable)
        }
    }
}

/// A PIN is exactly six ASCII digits.
#[must_use]
pub fn is_valid_pin(pin: &str) -> bool {
    pin.len() == 6 && pin.bytes().all(|b| b.is_ascii_digit())
}
