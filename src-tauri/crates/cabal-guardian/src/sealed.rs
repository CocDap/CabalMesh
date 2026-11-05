//! Sealed-box encryption to a guardian's known static public key — no live
//! session required.
//!
//! # Why not the mesh's own `Sealed` packet kind
//!
//! `docs/ble-mesh-design.md` describes a `Sealed` wire packet carrying
//! ciphertext inside a Noise XX session, and that session layer does not
//! exist in this codebase yet (no `session.rs`, no handshake code — see the
//! BLE-infrastructure survey behind this crate's guardian work). Waiting on
//! it would block guardian shares on an unrelated, larger piece of work.
//!
//! It also is not the right tool even once built: a Noise XX handshake
//! authenticates two parties who just met over the radio. A guardian is the
//! opposite case — enrolled ahead of time, with their public key already
//! known to the owner from that enrollment, and potentially out of radio
//! range when a share actually needs sending. What is needed is exactly
//! [libsodium's `crypto_box_seal`](https://doc.libsodium.org/public-key_cryptography/sealed_boxes)
//! pattern: anyone holding a recipient's static public key can seal a
//! message to them, with no round trip and no prior session.
//!
//! # Construction
//!
//! A fresh X25519 keypair is generated per message. Its secret half is
//! Diffie-Hellman'd against the recipient's static public key and then
//! **immediately consumed** — [`EphemeralSecret`] enforces this at the type
//! level, since `diffie_hellman` takes it by value. The shared point is
//! never used directly as a cipher key; it is hashed together with both
//! public keys and a domain-separation tag, which is what stops the same
//! shared secret from being reusable as a key for anything else that might
//! ever hash X25519 output the same way.
//!
//! The recipient learns nothing about who sent a sealed message beyond what
//! the plaintext itself says. For a guardian share, the plaintext is
//! expected to name the owner explicitly — anonymity of the *transport* is a
//! feature, but a guardian approving an unlock needs to know whose vault
//! they are approving, which is content the message payload carries, not
//! this layer.

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use rand_core::{OsRng, RngCore};
use sha2::{Digest, Sha256};
use x25519_dalek::{EphemeralSecret, PublicKey, StaticSecret};
use zeroize::Zeroize;

const DOMAIN: &[u8] = b"cabalmesh-guardian-sealed-v1";
const NONCE_LEN: usize = 12;
const PUBLIC_KEY_LEN: usize = 32;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SealedError {
    /// Too short to contain an ephemeral key and a nonce, let alone a tag.
    #[error("sealed payload is malformed")]
    Malformed,

    /// Wrong key, or the payload was tampered with. ChaCha20Poly1305
    /// authenticates, so those are indistinguishable and both mean the same
    /// thing: do not trust the plaintext, because there isn't one.
    #[error("could not open the sealed payload")]
    Decrypt,
}

/// A guardian's long-lived key pair, generated once at enrollment and kept
/// for as long as they hold a share.
///
/// Deliberately distinct from any BLE session identity (ephemeral, changes
/// every launch) and from the owner's NEAR signing key (ed25519, a
/// different curve, a different purpose). This key exists for exactly one
/// thing: receiving shares sealed to it.
pub struct GuardianSecretKey(StaticSecret);

impl GuardianSecretKey {
    /// A fresh key pair.
    #[must_use]
    pub fn generate() -> Self {
        Self(StaticSecret::random_from_rng(OsRng))
    }

    /// The half to hand to whoever will seal messages to this guardian.
    #[must_use]
    pub fn public_key(&self) -> GuardianPublicKey {
        GuardianPublicKey(PublicKey::from(&self.0))
    }

    /// Raw bytes, for persisting alongside the rest of a guardian's
    /// enrollment state.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; PUBLIC_KEY_LEN] {
        self.0.to_bytes()
    }

    /// Restores a previously generated key.
    #[must_use]
    pub fn from_bytes(bytes: [u8; PUBLIC_KEY_LEN]) -> Self {
        Self(StaticSecret::from(bytes))
    }
}

/// Deliberately opaque — this is key material once combined with a share.
impl std::fmt::Debug for GuardianSecretKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GuardianSecretKey(<redacted>)")
    }
}

/// The half of a guardian's key pair that is safe to share — sent to the
/// owner during enrollment, so they can seal shares to it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct GuardianPublicKey(PublicKey);

impl GuardianPublicKey {
    #[must_use]
    pub fn to_bytes(&self) -> [u8; PUBLIC_KEY_LEN] {
        self.0.to_bytes()
    }

    #[must_use]
    pub fn from_bytes(bytes: [u8; PUBLIC_KEY_LEN]) -> Self {
        Self(PublicKey::from(bytes))
    }
}

impl std::fmt::Debug for GuardianPublicKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "GuardianPublicKey({})", hex_encode(&self.to_bytes()))
    }
}

/// A message sealed to one [`GuardianPublicKey`]. Opaque to everyone else,
/// including whoever relays it across the mesh.
#[derive(Clone)]
pub struct Sealed {
    ephemeral_public_key: [u8; PUBLIC_KEY_LEN],
    nonce: [u8; NONCE_LEN],
    ciphertext: Vec<u8>,
}

/// Ciphertext, not plaintext — but still not printed. Nothing about a
/// `Sealed` value is useful in a log beyond its size, and printing raw
/// crypto bytes anywhere is a habit this codebase avoids consistently.
impl std::fmt::Debug for Sealed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sealed").field("ciphertext_len", &self.ciphertext.len()).finish()
    }
}

impl Sealed {
    /// Serializes to `ephemeral_public_key || nonce || ciphertext`, ready to
    /// go on the wire as a packet payload.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(PUBLIC_KEY_LEN + NONCE_LEN + self.ciphertext.len());
        out.extend_from_slice(&self.ephemeral_public_key);
        out.extend_from_slice(&self.nonce);
        out.extend_from_slice(&self.ciphertext);
        out
    }

    /// Parses bytes previously produced by [`Sealed::to_bytes`] (or
    /// [`seal`]).
    ///
    /// # Errors
    ///
    /// [`SealedError::Malformed`] if `bytes` is too short to contain a key
    /// and a nonce, let alone a ciphertext.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, SealedError> {
        if bytes.len() < PUBLIC_KEY_LEN + NONCE_LEN {
            return Err(SealedError::Malformed);
        }

        let mut ephemeral_public_key = [0_u8; PUBLIC_KEY_LEN];
        ephemeral_public_key.copy_from_slice(&bytes[..PUBLIC_KEY_LEN]);

        let mut nonce = [0_u8; NONCE_LEN];
        nonce.copy_from_slice(&bytes[PUBLIC_KEY_LEN..PUBLIC_KEY_LEN + NONCE_LEN]);

        let ciphertext = bytes[PUBLIC_KEY_LEN + NONCE_LEN..].to_vec();

        Ok(Self { ephemeral_public_key, nonce, ciphertext })
    }
}

/// Seals `plaintext` so that only the holder of `recipient`'s matching
/// [`GuardianSecretKey`] can read it.
#[must_use]
pub fn seal(plaintext: &[u8], recipient: &GuardianPublicKey) -> Sealed {
    let ephemeral_secret = EphemeralSecret::random_from_rng(OsRng);
    let ephemeral_public = PublicKey::from(&ephemeral_secret);
    let shared = ephemeral_secret.diffie_hellman(&recipient.0);

    let mut key_bytes = derive_key(shared.as_bytes(), ephemeral_public.as_bytes(), recipient.0.as_bytes());
    let cipher = ChaCha20Poly1305::new((&key_bytes).into());
    key_bytes.zeroize();

    let mut nonce_bytes = [0_u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce_bytes);

    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), plaintext)
        // Only reachable with a plaintext long enough to overflow the AEAD's
        // internal counter, which a Shamir share or an unlock message never
        // approaches.
        .expect("chacha20poly1305 encryption should not fail for share-sized payloads");

    Sealed { ephemeral_public_key: ephemeral_public.to_bytes(), nonce: nonce_bytes, ciphertext }
}

/// Opens a [`Sealed`] payload with the matching [`GuardianSecretKey`].
///
/// # Errors
///
/// [`SealedError::Decrypt`] if `recipient_secret` is not the key this was
/// sealed to, or the payload was tampered with in transit.
pub fn open(sealed: &Sealed, recipient_secret: &GuardianSecretKey) -> Result<Vec<u8>, SealedError> {
    let ephemeral_public = PublicKey::from(sealed.ephemeral_public_key);
    let shared = recipient_secret.0.diffie_hellman(&ephemeral_public);
    let recipient_public = PublicKey::from(&recipient_secret.0);

    let mut key_bytes = derive_key(shared.as_bytes(), ephemeral_public.as_bytes(), recipient_public.as_bytes());
    let cipher = ChaCha20Poly1305::new((&key_bytes).into());
    key_bytes.zeroize();

    let plaintext = cipher
        .decrypt(Nonce::from_slice(&sealed.nonce), sealed.ciphertext.as_ref())
        .map_err(|_| SealedError::Decrypt)?;
    Ok(plaintext)
}

/// The shared X25519 point is never used as a key directly: it is hashed
/// together with a domain tag and both public keys, so this key cannot be
/// confused with a key derived the same way for some unrelated purpose, and
/// so sender and recipient are both bound into what was actually agreed on.
fn derive_key(shared_secret: &[u8; 32], ephemeral_public: &[u8; 32], recipient_public: &[u8; 32]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(DOMAIN);
    hasher.update(shared_secret);
    hasher.update(ephemeral_public);
    hasher.update(recipient_public);
    hasher.finalize().into()
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_then_open_round_trips() {
        let guardian = GuardianSecretKey::generate();
        let sealed = seal(b"a Shamir share's bytes", &guardian.public_key());

        let opened = open(&sealed, &guardian).unwrap();
        assert_eq!(opened, b"a Shamir share's bytes");
    }

    #[test]
    fn the_wrong_key_cannot_open_it() {
        let guardian = GuardianSecretKey::generate();
        let impostor = GuardianSecretKey::generate();
        let sealed = seal(b"secret", &guardian.public_key());

        assert!(matches!(open(&sealed, &impostor), Err(SealedError::Decrypt)));
    }

    #[test]
    fn tampering_with_the_ciphertext_is_detected() {
        let guardian = GuardianSecretKey::generate();
        let mut sealed = seal(b"secret", &guardian.public_key());
        let bytes = sealed.to_bytes();
        let mut tampered = bytes.clone();
        *tampered.last_mut().unwrap() ^= 0xff;
        sealed = Sealed::from_bytes(&tampered).unwrap();

        assert!(matches!(open(&sealed, &guardian), Err(SealedError::Decrypt)));
    }

    #[test]
    fn tampering_with_the_ephemeral_key_is_detected() {
        // The ephemeral key is bound into the derived key via `derive_key`,
        // not just carried alongside the ciphertext — flipping a bit in it
        // must not merely fail to decrypt "by luck" via a wrong DH output,
        // it must fail via the AEAD tag either way.
        let guardian = GuardianSecretKey::generate();
        let sealed = seal(b"secret", &guardian.public_key());
        let mut bytes = sealed.to_bytes();
        bytes[0] ^= 0xff;
        let tampered = Sealed::from_bytes(&bytes).unwrap();

        assert!(matches!(open(&tampered, &guardian), Err(SealedError::Decrypt)));
    }

    #[test]
    fn shares_round_trip_through_bytes() {
        let guardian = GuardianSecretKey::generate();
        let sealed = seal(b"share payload", &guardian.public_key());
        let round_tripped = Sealed::from_bytes(&sealed.to_bytes()).unwrap();

        assert_eq!(open(&round_tripped, &guardian).unwrap(), b"share payload");
    }

    #[test]
    fn a_truncated_payload_is_rejected_not_a_panic() {
        assert!(matches!(Sealed::from_bytes(&[1, 2, 3]), Err(SealedError::Malformed)));
    }

    #[test]
    fn sealing_the_same_plaintext_twice_produces_different_bytes() {
        // Fresh ephemeral key and nonce every call — a passive observer must
        // not be able to tell two sealed messages carry the same plaintext.
        let guardian = GuardianSecretKey::generate();
        let first = seal(b"same plaintext", &guardian.public_key());
        let second = seal(b"same plaintext", &guardian.public_key());

        assert_ne!(first.to_bytes(), second.to_bytes());
        assert_eq!(open(&first, &guardian).unwrap(), open(&second, &guardian).unwrap());
    }

    #[test]
    fn a_guardian_key_survives_a_byte_round_trip() {
        // Persisted to disk between app launches, so this is the shape that
        // actually gets exercised, not the in-memory value.
        let guardian = GuardianSecretKey::generate();
        let restored = GuardianSecretKey::from_bytes(guardian.to_bytes());

        assert_eq!(guardian.public_key().to_bytes(), restored.public_key().to_bytes());

        let sealed = seal(b"secret", &guardian.public_key());
        assert_eq!(open(&sealed, &restored).unwrap(), b"secret");
    }
}
