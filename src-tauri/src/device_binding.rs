//! The device-bound half of the vault key.
//!
//! # What this is for, and what it is not
//!
//! The passphrase closes the at-rest hole: a copied key file is ciphertext.
//! This layer closes a different one — it makes that copied file useless *on
//! another machine*, even to someone who also learned the passphrase, because
//! part of the key never left the device it was made on.
//!
//! It is a layer **under** the passphrase, never a replacement for it. That
//! ordering is not stylistic. On every desktop key store that unlocks once per
//! session, any process running as this user can simply ask the store for the
//! item; a design that leaned on hardware alone would hand the key to exactly
//! the attacker the passphrase was introduced to stop.
//!
//! # What each platform actually provides
//!
//! | Platform | Store | Wired | What it buys |
//! |---|---|---|---|
//! | macOS, iOS | Keychain | yes | The secret is not in the app's files at all. A keychain ACL binds the item to the signed application, so on a signed build another process must prompt the user; on an unsigned or ad-hoc build it does not, and this module does not claim it does. |
//! | Android | Keystore / StrongBox | yes | The secret is encrypted under a non-exportable AES key held in StrongBox where the device has one and in the TEE otherwise. The blob travels with a copied file; the key that opens it cannot leave the device. See `tauri-plugin-cabal-keystore`. |
//! | Windows | TPM, DPAPI | **no** | DPAPI would add nothing against a same-user process, and a TPM-sealed secret is real work that has not been done. |
//! | Linux | — | n/a | No store that survives the headless case, which is why `keyring` was removed. Reported as absent. |
//!
//! # The honest limit
//!
//! Without a user-presence requirement, hardware binding stops *exfiltration*
//! rather than *access*: code running as this user, on this machine, while the
//! app can run, can still obtain the secret. Requiring presence on every
//! unlock is a product decision with a real cost, recorded as open in
//! `docs/identity-design.md` rather than made silently here.

use cabal_vault::VaultError;
use zeroize::Zeroizing;

/// The name the secret is stored under. Stable: changing it strands every
/// vault written before the change.
#[cfg(any(target_os = "macos", target_os = "ios"))]
const KEYCHAIN_SERVICE: &str = "com.cabalmesh.vault";
#[cfg(any(target_os = "macos", target_os = "ios"))]
const KEYCHAIN_ACCOUNT: &str = "vault-device-secret";

/// Which store produced the device secret, recorded in the key envelope.
///
/// Written into the file so a vault made *with* a binding is refused with a
/// clear reason on a machine that has none, rather than failing as if the
/// passphrase were wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Binding {
    /// No device secret. The passphrase is the only factor.
    None,
    AppleKeychain,
    AndroidKeystore,
}

impl Binding {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::AppleKeychain => "apple-keychain",
            Self::AndroidKeystore => "android-keystore",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "none" => Some(Self::None),
            "apple-keychain" => Some(Self::AppleKeychain),
            "android-keystore" => Some(Self::AndroidKeystore),
            _ => None,
        }
    }
}

/// Why this platform has the binding it has.
///
/// Three states rather than two, because "this platform has no key store" and
/// "this build has not connected the key store this platform has" are
/// different facts, and collapsing them into "unavailable" would let the
/// second hide behind the first indefinitely.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    /// A store exists and is in use.
    Wired,
    /// Hardware exists; this build does not use it.
    NotWired,
    /// The platform has nothing suitable.
    Absent,
}

/// The store this platform actually has, ignoring test configuration.
///
/// On Android this is conditional on the plugin having registered. A build
/// where registration failed must not claim a binding it cannot produce — it
/// would write `android-keystore` into an envelope it can never open again.
#[must_use]
pub fn platform_store() -> Binding {
    if cfg!(any(target_os = "macos", target_os = "ios")) {
        Binding::AppleKeychain
    } else if cfg!(target_os = "android") && android_source_installed() {
        Binding::AndroidKeystore
    } else {
        Binding::None
    }
}

/// What the key store does while the unit tests run.
///
/// The real store is off by default. The suite would otherwise create and read
/// login-keychain items on every run of every vault test, which prompts on
/// machines where the keychain is locked or the test binary is unsigned —
/// turning `cargo test` into something that needs a human. The real path is
/// covered by the `--ignored` test that opts in.
///
/// The other modes are not merely stand-ins for the store. They are how the
/// two failures this layer exists to produce get tested at all: a key file
/// carried to a machine with no store, and one carried to a *different*
/// machine that has its own.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TestStore {
    /// A fixed secret, as if this machine's store held one.
    ThisDevice,
    /// A different fixed secret: the same file opened on another device.
    AnotherDevice,
    /// No store at all, as on Linux.
    Unavailable,
    /// The real platform store.
    Real,
}

// Per-thread, not global: the test harness gives each test its own thread,
// and a shared mode would mean one test's "I am another device" arrived in the
// middle of another test's unlock.
#[cfg(test)]
thread_local! {
    static TEST_STORE: std::cell::Cell<TestStore> = const { std::cell::Cell::new(TestStore::ThisDevice) };
}

#[cfg(test)]
fn test_store() -> TestStore {
    TEST_STORE.with(std::cell::Cell::get)
}

/// Runs `body` with the key store answering as `store`, on this thread only.
#[cfg(test)]
pub(crate) fn with_test_store<T>(store: TestStore, body: impl FnOnce() -> T) -> T {
    let previous = TEST_STORE.with(|cell| cell.replace(store));
    let outcome = body();
    TEST_STORE.with(|cell| cell.set(previous));
    outcome
}

/// This platform's binding.
#[must_use]
pub fn platform_binding() -> Binding {
    platform_store()
}

/// Why, in one word the interface can render.
#[must_use]
pub fn availability() -> Availability {
    if cfg!(any(target_os = "macos", target_os = "ios")) {
        Availability::Wired
    } else if cfg!(target_os = "android") {
        if android_source_installed() {
            Availability::Wired
        } else {
            // The plugin exists but did not register on this run. A gap in
            // this build, not a fact about Android.
            Availability::NotWired
        }
    } else if cfg!(target_os = "windows") {
        // A TPM this build has not connected. Saying "absent" would be a lie
        // that ages badly.
        Availability::NotWired
    } else {
        Availability::Absent
    }
}

/// A sentence describing the real protection on the running platform.
///
/// Rendered verbatim by the vault screen, so it must never describe a better
/// platform than the one it is running on.
#[must_use]
pub fn describe() -> &'static str {
    if cfg!(any(target_os = "macos", target_os = "ios")) {
        "DEVICE-BOUND VIA KEYCHAIN. A COPIED FILE WILL NOT OPEN ELSEWHERE."
    } else if cfg!(target_os = "android") {
        if android_source_installed() {
            "DEVICE-BOUND VIA ANDROID KEYSTORE. A COPIED FILE WILL NOT OPEN ELSEWHERE."
        } else {
            "NOT DEVICE-BOUND. THE KEYSTORE PLUGIN DID NOT REGISTER ON THIS RUN."
        }
    } else if cfg!(target_os = "windows") {
        "NOT DEVICE-BOUND. TPM BINDING IS NOT WIRED IN THIS BUILD."
    } else {
        "NOT DEVICE-BOUND. THIS PLATFORM HAS NO KEY STORE TO BIND TO."
    }
}

/// Fetches the device secret for `binding`, creating it on first use.
///
/// `Ok(None)` means the binding is [`Binding::None`] and there is nothing to
/// mix in. `Err` means a binding was required and could not be obtained — the
/// caller must refuse rather than silently derive a key without it, because a
/// key derived without the secret is a *different* key and would present a
/// readable vault as a wrong passphrase.
pub fn secret(binding: Binding) -> Result<Option<Zeroizing<Vec<u8>>>, VaultError> {
    if binding == Binding::None {
        return Ok(None);
    }

    #[cfg(test)]
    match test_store() {
        TestStore::ThisDevice => return Ok(Some(Zeroizing::new(vec![0xA1; 32]))),
        TestStore::AnotherDevice => return Ok(Some(Zeroizing::new(vec![0xB2; 32]))),
        TestStore::Unavailable => return Err(VaultError::KeyUnavailable),
        TestStore::Real => {}
    }

    match binding {
        Binding::None => Ok(None),
        Binding::AppleKeychain => apple_keychain_secret().map(Some),
        Binding::AndroidKeystore => android_keystore_secret().map(Some),
    }
}

/// How the Android Keystore is reached from code that has no app handle.
///
/// The vault key provider is built deep inside `IdentityStore`, which is
/// constructed before — and independently of — anything holding a `Manager`.
/// Threading a handle down to it would mean changing every constructor on the
/// way for one platform's benefit. Installing a source once at startup is the
/// same shape `app_paths` already uses, and for the same reason.
type AndroidSource = Box<dyn Fn() -> Result<Vec<u8>, String> + Send + Sync>;

static ANDROID_SOURCE: std::sync::OnceLock<AndroidSource> = std::sync::OnceLock::new();

/// Installs the Keystore source. Called once, at startup, after the plugin
/// registers. A second call is ignored rather than replacing the first.
pub fn install_android_source(source: AndroidSource) {
    if ANDROID_SOURCE.set(source).is_err() {
        tracing::warn!(
            target: "cabalmesh::vault",
            "the Android Keystore source was already installed; keeping the first"
        );
    }
}

#[must_use]
fn android_source_installed() -> bool {
    ANDROID_SOURCE.get().is_some()
}

fn android_keystore_secret() -> Result<Zeroizing<Vec<u8>>, VaultError> {
    let Some(source) = ANDROID_SOURCE.get() else {
        // Reachable by opening an Android-bound key file on any other
        // platform — a copied file. Refusing is the demonstration that the
        // binding works.
        tracing::error!(
            target: "cabalmesh::vault",
            "this key is bound to an Android Keystore, which is not available here"
        );
        return Err(VaultError::KeyUnavailable);
    };

    match source() {
        Ok(secret) if secret.len() == 32 => Ok(Zeroizing::new(secret)),
        Ok(_) => {
            tracing::error!(target: "cabalmesh::vault", "the Android Keystore returned a malformed secret");
            Err(VaultError::KeyUnavailable)
        }
        Err(error) => {
            tracing::error!(target: "cabalmesh::vault", %error, "the Android Keystore refused");
            Err(VaultError::KeyUnavailable)
        }
    }
}

/// A value that identifies *which* device secret was used, without revealing
/// it.
///
/// Stored in the key envelope so the same file carried to a different machine
/// — one that has a store of its own, holding a different secret — is refused
/// as the wrong device rather than as the wrong passphrase. Without this the
/// two are indistinguishable: both produce a key that does not decrypt, and
/// the user is sent to retype a passphrase that was correct all along.
#[must_use]
pub fn fingerprint(salt: &[u8], device_secret: Option<&[u8]>) -> Vec<u8> {
    let Some(device_secret) = device_secret else {
        return Vec::new();
    };
    let mut input = Vec::with_capacity(salt.len() + device_secret.len() + 1);
    input.extend_from_slice(b"cabalmesh-device-binding-v1");
    input.extend_from_slice(salt);
    input.extend_from_slice(device_secret);
    // Keccak rather than SHA-3 because this is what it has always been: changing
    // it would change the fingerprint of every envelope already on disk.
    use sha3::Digest as _;
    sha3::Keccak256::digest(&input)[..16].to_vec()
}

/// The binding to use for a **new** vault, and the secret that goes with it.
///
/// Falls back to [`Binding::None`] when the platform store refuses. That
/// matters more than it looks: a build that is not codesigned, a machine whose
/// login keychain is locked, or a CI runner with no keychain at all would
/// otherwise be unable to create a vault of any kind — the extra layer would
/// have become a requirement for the app to run.
///
/// The fallback is recorded in the envelope and surfaced in the interface, so
/// a vault that is not device-bound never presents itself as one that is.
pub fn for_new_vault() -> (Binding, Option<Zeroizing<Vec<u8>>>) {
    let preferred = platform_binding();
    match secret(preferred) {
        Ok(secret) => (preferred, secret),
        Err(_) => {
            tracing::warn!(
                target: "cabalmesh::vault",
                store = preferred.as_str(),
                "the device key store refused; this vault will be protected by its passphrase alone"
            );
            (Binding::None, None)
        }
    }
}

/// Serializes first-run creation.
///
/// Without it, two callers that both find no item both create one and the
/// second overwrites the first — leaving a vault wrapped under a secret that
/// no longer exists. The keychain has no compare-and-set, so the ordering has
/// to be imposed here.
#[cfg(any(target_os = "macos", target_os = "ios"))]
static KEYCHAIN_CREATE: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn apple_keychain_secret() -> Result<Zeroizing<Vec<u8>>, VaultError> {
    use security_framework::passwords::{get_generic_password, set_generic_password};

    fn stored() -> Option<Result<Zeroizing<Vec<u8>>, VaultError>> {
        let existing = get_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT).ok()?;
        if existing.len() == 32 {
            return Some(Ok(Zeroizing::new(existing)));
        }
        // A wrong-length item is not something to overwrite: it may be the
        // only copy of a secret written by a build that used a different
        // length, and replacing it would orphan that vault.
        tracing::error!(
            target: "cabalmesh::vault",
            "the keychain item for this vault is not the expected size; refusing to replace it"
        );
        Some(Err(VaultError::KeyUnavailable))
    }

    if let Some(result) = stored() {
        return result;
    }

    // Poisoning only means a previous creator panicked mid-way; the recheck
    // below is what decides, so the lock is still usable.
    let _guard = KEYCHAIN_CREATE.lock().unwrap_or_else(|error| error.into_inner());

    // Re-read under the lock: another caller may have created it while this
    // one waited, and its secret is now the real one.
    if let Some(result) = stored() {
        return result;
    }

    let mut fresh = [0_u8; 32];
    use aes_gcm::aead::rand_core::RngCore;
    aes_gcm::aead::OsRng
        .try_fill_bytes(&mut fresh)
        .map_err(|_| VaultError::KeyUnavailable)?;

    set_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT, &fresh).map_err(|error| {
        tracing::error!(target: "cabalmesh::vault", %error, "could not store the device secret in the keychain");
        VaultError::KeyUnavailable
    })?;
    tracing::info!(target: "cabalmesh::vault", "created a device-bound secret in the keychain");

    // Read back rather than returning what was just generated. Another process
    // — a second copy of this app — could have written between the check and
    // the store, and the value that survived is the one every envelope on this
    // machine has to be wrapped under.
    stored().unwrap_or(Err(VaultError::KeyUnavailable))
}

#[cfg(not(any(target_os = "macos", target_os = "ios")))]
fn apple_keychain_secret() -> Result<Zeroizing<Vec<u8>>, VaultError> {
    // Reachable only by opening a vault written on an Apple device on a
    // platform that has no keychain — a copied key file. Refusing is the
    // demonstration that the binding works.
    tracing::error!(
        target: "cabalmesh::vault",
        "this key is bound to an Apple keychain, which this platform does not have"
    );
    Err(VaultError::KeyUnavailable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_binding_name_round_trips() {
        // The envelope stores this string. A rename that did not round trip
        // would make every existing vault unopenable.
        for binding in [Binding::None, Binding::AppleKeychain, Binding::AndroidKeystore] {
            assert_eq!(Binding::parse(binding.as_str()), Some(binding));
        }
    }

    #[test]
    fn an_unknown_binding_is_not_guessed() {
        // A newer build may write a binding this one does not know. Guessing
        // would derive a wrong key and blame the passphrase.
        assert_eq!(Binding::parse("windows-tpm"), None);
    }

    #[test]
    fn no_binding_yields_no_secret() {
        assert!(secret(Binding::None).unwrap().is_none());
    }

    #[test]
    fn a_fingerprint_identifies_the_device_without_revealing_it() {
        let salt = [3_u8; 16];
        let this = secret(Binding::AppleKeychain).unwrap().unwrap();
        let mine = fingerprint(&salt, Some(&this));

        assert_eq!(mine.len(), 16);
        assert!(
            !this.windows(4).any(|window| mine.windows(4).any(|other| window == other)),
            "the fingerprint leaks the secret it identifies"
        );
        assert_eq!(mine, fingerprint(&salt, Some(&this)), "not stable");
        assert_ne!(mine, fingerprint(&[4_u8; 16], Some(&this)), "salt is not mixed in");
        assert_ne!(mine, fingerprint(&salt, Some(&[0xB2; 32])), "another device matched");
        assert!(fingerprint(&salt, None).is_empty(), "an unbound key claimed a device");
    }

    #[test]
    fn a_new_vault_is_always_creatable() {
        // Whatever the store does — grant, refuse, or not exist — creating a
        // vault must remain possible. An extra layer that can block the app
        // from starting is not an extra layer, it is a new failure mode.
        let (binding, secret) = for_new_vault();
        assert_eq!(secret.is_some(), binding != Binding::None);
    }

    #[test]
    fn the_description_matches_the_platform_it_runs_on() {
        // The row this renders sits next to a wallet. It must describe the
        // machine the user is holding, not the best platform in the table.
        let described = describe();
        match availability() {
            Availability::Wired => assert!(described.contains("DEVICE-BOUND VIA")),
            Availability::NotWired => assert!(described.contains("NOT WIRED")),
            Availability::Absent => assert!(described.contains("NO KEY STORE")),
        }
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    #[test]
    fn apple_platforms_are_wired() {
        // The platform answer, not the test-gated one: what ships is what
        // matters here.
        assert_eq!(platform_store(), Binding::AppleKeychain);
        assert_eq!(availability(), Availability::Wired);
    }

    #[cfg(target_os = "android")]
    #[test]
    fn android_reports_a_gap_rather_than_an_absence() {
        // Registration happens at startup, which a unit test does not run.
        // What matters is that the two answers agree: an uninstalled source
        // must never be reported as a wired binding.
        assert_eq!(android_source_installed(), false);
        assert_eq!(availability(), Availability::NotWired);
        assert_eq!(platform_store(), Binding::None);
    }

    /// Touches the real login keychain, which is not available on every
    /// machine a developer or CI runner uses — an unsigned build can be
    /// refused, and a locked keychain prompts. Run it deliberately:
    /// `cargo test --lib device_binding -- --ignored`.
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    #[test]
    #[ignore = "requires access to the login keychain"]
    fn the_device_secret_is_stable_and_never_empty() {
        TEST_STORE.with(|cell| cell.set(TestStore::Real));
        let first = secret(Binding::AppleKeychain).unwrap().unwrap();
        let second = secret(Binding::AppleKeychain).unwrap().unwrap();

        assert_eq!(first.len(), 32);
        assert_ne!(first.to_vec(), vec![0_u8; 32]);
        assert_eq!(
            first.to_vec(),
            second.to_vec(),
            "a device secret that changed would orphan the vault it protects"
        );
    }
}
