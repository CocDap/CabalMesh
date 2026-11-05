//! Crypto for the mesh-guardian recovery scheme in
//! `docs/identity-design.md`: splitting the vault's data key into shares
//! ([`split`]/[`reconstruct`], this module) and sealing a share to one
//! guardian's public key ([`sealed`]).
//!
//! # Scope
//!
//! This crate is the crypto core only. It has no I/O and knows nothing about
//! BLE transport, guardians as enrolled peers, persistence, or the unlock
//! request/approve protocol — those are a separate, larger piece of work
//! (see the doc's mesh-unlock design) that this crate exists to make
//! buildable in a later, focused pass.
//!
//! # Why a vetted crate instead of hand-rolled math
//!
//! Everything else new in this session (the vault, the passphrase provider)
//! is written in-house, matching this codebase's general preference for
//! owning small pieces over taking a dependency. Shamir's scheme is the
//! exception: a subtle bug in polynomial interpolation or coefficient
//! randomness is the kind of mistake that fails silently — the shares look
//! fine, reconstruction "works," and the key is subtly wrong or predictable.
//! [`sharks`] is small, has no unsafe code, and ships a `zeroize` feature
//! that matches the hygiene the rest of this codebase already holds key
//! material to.
//!
//! # The property every caller must hold onto
//!
//! Shamir's scheme has **no built-in integrity check**. Reconstructing from
//! shares that don't actually belong together — a stale share, a forged
//! one, shares from two different splits — does not error. It silently
//! produces a different 32 bytes. See [`reconstruct`]'s docs for what
//! callers are responsible for because of this.

#![forbid(unsafe_code)]

pub mod protocol;
pub mod sealed;

use cabal_vault::DataKey;
use sharks::Sharks;
use std::convert::TryFrom;
use zeroize::Zeroize;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum GuardianError {
    /// `k` was below 2 (a lone guardian could unlock alone) or above `n`
    /// (the threshold could never be met).
    #[error("threshold must be at least 2, and at most the number of guardians")]
    InvalidThreshold,

    /// Fewer than `k` distinct shares were supplied. This is the *only*
    /// failure [`reconstruct`] can detect — see its docs.
    #[error("need at least {needed} shares to recover the key, got {got}")]
    InsufficientShares { needed: u8, got: usize },

    /// A share was too short, or not something [`split`] produced.
    #[error("a share is malformed")]
    MalformedShare,
}

/// One guardian's piece of a split vault key.
///
/// Opaque bytes with no meaning on their own — a single share reveals
/// nothing about the key it came from. [`Share::to_bytes`] is what a caller
/// encrypts to a guardian's public key and sends; nothing in this crate
/// does that encryption, since that belongs to the transport layer this
/// crate deliberately does not depend on.
#[derive(Clone)]
pub struct Share(sharks::Share);

impl Share {
    /// Serializes to bytes, ready to be encrypted and transmitted.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        Vec::from(&self.0)
    }

    /// Parses a share previously produced by [`split`].
    ///
    /// # Errors
    ///
    /// [`GuardianError::MalformedShare`] if `bytes` is too short to be a
    /// share this crate produced. This is a shape check only — it cannot
    /// tell a share is wrong-but-well-formed; see [`reconstruct`].
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, GuardianError> {
        sharks::Share::try_from(bytes).map(Share).map_err(|_| GuardianError::MalformedShare)
    }
}

/// Deliberately opaque, matching [`DataKey`]'s own `Debug` — a share is key
/// material once enough of its siblings are around, and must not print.
impl std::fmt::Debug for Share {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Share(<redacted>)")
    }
}

/// Splits `key` into `n` shares, any `k` of which reconstruct it.
///
/// # Errors
///
/// [`GuardianError::InvalidThreshold`] if `k < 2` or `k > n`.
pub fn split(key: &DataKey, k: u8, n: u8) -> Result<Vec<Share>, GuardianError> {
    if k < 2 || k > n {
        return Err(GuardianError::InvalidThreshold);
    }

    let sharks = Sharks(k);
    let mut bytes = key.expose_for_storage();
    let shares = sharks.dealer(&bytes).take(usize::from(n)).map(Share).collect();

    // A second plaintext of the key; do not leave it in freed memory.
    bytes.zeroize();
    Ok(shares)
}

/// Reconstructs a candidate vault key from `shares`, given the threshold
/// `k` they were split with.
///
/// **This does not prove the shares were genuine, or that they came from
/// the same split.** Shamir's scheme has no integrity check: fewer than `k`
/// distinct shares fails cleanly (below), but `k` or more well-formed shares
/// that don't actually belong together — a stale share from a removed
/// guardian, a forged one, shares mixed from two different owners —
/// interpolate into a *different* 32 bytes with no error at all, because
/// the math has no way to know they weren't meant to combine.
///
/// The only thing that actually proves a reconstructed key is right is the
/// same thing that already proves any vault key right: `Vault::load`
/// succeeding, because AES-GCM authenticates and Shamir does not. Treat
/// this function's output as a candidate key, never a verified one.
///
/// # Errors
///
/// [`GuardianError::InsufficientShares`] if fewer than `k` distinct shares
/// are given. [`GuardianError::MalformedShare`] if the shares recombine to
/// something other than 32 bytes — they were never a valid split of a
/// [`DataKey`] to begin with.
pub fn reconstruct(k: u8, shares: &[Share]) -> Result<DataKey, GuardianError> {
    let sharks = Sharks(k);
    let mut secret = sharks.recover(shares.iter().map(|s| &s.0)).map_err(|_| {
        GuardianError::InsufficientShares { needed: k, got: shares.len() }
    })?;

    if secret.len() != 32 {
        secret.zeroize();
        return Err(GuardianError::MalformedShare);
    }

    let mut bytes = [0_u8; 32];
    bytes.copy_from_slice(&secret);
    secret.zeroize();

    let key = DataKey::from_bytes(bytes);
    bytes.zeroize();
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(fill: u8) -> DataKey {
        DataKey::from_bytes([fill; 32])
    }

    #[test]
    fn any_k_of_n_shares_recover_the_original_key() {
        let original = key(7);
        let shares = split(&original, 3, 5).unwrap();

        for subset in [
            &shares[0..3],
            &shares[1..4],
            &shares[2..5],
            &[shares[0].clone(), shares[2].clone(), shares[4].clone()][..],
        ] {
            let recovered = reconstruct(3, subset).unwrap();
            assert_eq!(
                recovered.expose_for_storage(),
                original.expose_for_storage(),
                "failed to recover from a valid subset of shares"
            );
        }
    }

    #[test]
    fn fewer_than_the_threshold_is_refused() {
        let shares = split(&key(1), 3, 5).unwrap();

        assert!(matches!(
            reconstruct(3, &shares[0..2]),
            Err(GuardianError::InsufficientShares { needed: 3, got: 2 })
        ));
    }

    #[test]
    fn zero_shares_is_refused_not_a_panic() {
        assert!(matches!(
            reconstruct(3, &[]),
            Err(GuardianError::InsufficientShares { needed: 3, got: 0 })
        ));
    }

    #[test]
    fn a_threshold_of_one_is_rejected() {
        // A lone guardian must never be able to reconstruct the key alone —
        // that would defeat the entire point of spreading trust.
        assert!(matches!(split(&key(1), 1, 5), Err(GuardianError::InvalidThreshold)));
    }

    #[test]
    fn a_threshold_above_the_share_count_is_rejected() {
        // A threshold that could never be met is a vault nobody can ever
        // reopen, which is worse than refusing to create it.
        assert!(matches!(split(&key(1), 6, 5), Err(GuardianError::InvalidThreshold)));
    }

    #[test]
    fn shares_round_trip_through_bytes() {
        let shares = split(&key(9), 3, 5).unwrap();
        let round_tripped: Vec<Share> =
            shares.iter().map(|s| Share::from_bytes(&s.to_bytes()).unwrap()).collect();

        let recovered = reconstruct(3, &round_tripped).unwrap();
        assert_eq!(recovered.expose_for_storage(), key(9).expose_for_storage());
    }

    #[test]
    fn a_share_too_short_to_be_real_is_rejected() {
        assert!(matches!(Share::from_bytes(&[1]), Err(GuardianError::MalformedShare)));
    }

    #[test]
    fn mixed_shares_from_two_different_keys_do_not_error_but_do_not_recover_either_key() {
        // The property `reconstruct`'s docs exist to warn about: enough
        // well-formed shares combine into *something* even when they were
        // never part of the same split. This is exactly why a caller must
        // verify the result by actually opening the vault, not by trusting
        // that `reconstruct` returned `Ok`.
        let key_a = key(0xAA);
        let key_b = key(0xBB);
        let shares_a = split(&key_a, 3, 5).unwrap();
        let shares_b = split(&key_b, 3, 5).unwrap();

        let mixed = vec![shares_a[0].clone(), shares_a[1].clone(), shares_b[2].clone()];
        let recovered = reconstruct(3, &mixed).unwrap();

        assert_ne!(recovered.expose_for_storage(), key_a.expose_for_storage());
        assert_ne!(recovered.expose_for_storage(), key_b.expose_for_storage());
    }

    #[test]
    fn two_splits_of_the_same_key_produce_different_share_bytes() {
        // `dealer` draws fresh random polynomial coefficients every call, so
        // shares are not reusable across enrollments even for an unchanged
        // key — a leaked historical share set must not still work.
        let original = key(3);
        let first = split(&original, 3, 5).unwrap();
        let second = split(&original, 3, 5).unwrap();

        assert_ne!(first[0].to_bytes(), second[0].to_bytes());
        // Both still recover the same key on their own.
        assert_eq!(
            reconstruct(3, &first[0..3]).unwrap().expose_for_storage(),
            reconstruct(3, &second[0..3]).unwrap().expose_for_storage()
        );
    }
}
