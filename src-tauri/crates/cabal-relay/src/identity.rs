//! The relay's identity keypair.
//!
//! # This key is load-bearing in a way most keys are not
//!
//! The relay's peer identifier is compiled into every shipped build as part of
//! its bootstrap address. Rotating it strands every installed app until those
//! users update; losing it strands them permanently. It is closer to a signing
//! key than to a session key, and it is generated once and never again.
//!
//! Which is why this module will **create** a key only when explicitly asked.
//! A relay that silently generates a fresh identity when its key file is
//! missing is the worst possible failure: it starts cleanly, logs nothing
//! alarming, and every phone in the world quietly stops being able to reserve.
//! A relay whose key is missing refuses to start.

use libp2p::identity::Keypair;
use std::path::Path;

/// Why an identity could not be loaded.
#[derive(Debug)]
pub enum IdentityError {
    /// The file is not there. **Not** an invitation to generate one.
    Missing,
    /// The file is there and unreadable.
    Unreadable(std::io::Error),
    /// The bytes are not a keypair.
    Malformed,
}

impl std::fmt::Display for IdentityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing => f.write_str(
                "no identity key at that path. Generate one with `cabal-relay --generate-key <path>` \
                 and back it up off-host before using it — its peer id is compiled into shipped builds",
            ),
            Self::Unreadable(error) => write!(f, "identity key could not be read: {error}"),
            Self::Malformed => f.write_str("identity key is not a valid ed25519 keypair"),
        }
    }
}

impl std::error::Error for IdentityError {}

/// Loads the keypair at `path`.
///
/// # Errors
///
/// [`IdentityError::Missing`] rather than generating. See the module docs: a
/// relay that invents an identity on a missing file strands every installed
/// app without appearing to fail.
pub fn load(path: &Path) -> Result<Keypair, IdentityError> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(IdentityError::Missing)
        }
        Err(error) => return Err(IdentityError::Unreadable(error)),
    };

    Keypair::from_protobuf_encoding(&bytes).map_err(|_| IdentityError::Malformed)
}

/// Generates a keypair and writes it to `path`.
///
/// Refuses to overwrite. Clobbering a relay identity is unrecoverable — there
/// is no way to reconstruct the old one — so the only safe behaviour is to make
/// it impossible by accident.
///
/// # Errors
///
/// Any I/O failure, including the file already existing.
pub fn generate(path: &Path) -> Result<Keypair, std::io::Error> {
    if path.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "an identity already exists at that path; refusing to overwrite it. \
             Its peer id is compiled into shipped builds and cannot be reconstructed",
        ));
    }

    let keypair = Keypair::generate_ed25519();
    let encoded = keypair
        .to_protobuf_encoding()
        .map_err(|error| std::io::Error::other(error.to_string()))?;

    std::fs::write(path, encoded)?;
    restrict(path)?;

    Ok(keypair)
}

/// Makes the key file readable only by its owner.
///
/// A relay key sitting at 0644 on a shared host is a key anyone with an account
/// can copy, and copying it is enough to impersonate the relay to every phone.
#[cfg(unix)]
fn restrict(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn restrict(_path: &Path) -> std::io::Result<()> {
    // Windows ACLs are not a chmod, and the relay's supported platform is
    // Linux. Doing nothing beats doing something that looks like a permission
    // change and is not one.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_key_is_an_error_not_a_new_identity() {
        // The single most important behaviour in this file. Generating here
        // would strand every installed app, silently, on a relay that looks
        // healthy.
        let dir = std::env::temp_dir().join(format!("cabal-relay-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("absent.key");

        assert!(matches!(load(&path), Err(IdentityError::Missing)));
        assert!(!path.exists(), "loading must not create a key");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_generated_key_round_trips() {
        let dir = std::env::temp_dir().join(format!("cabal-relay-rt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("relay.key");

        let generated = generate(&path).unwrap();
        let loaded = load(&path).unwrap();

        assert_eq!(generated.public().to_peer_id(), loaded.public().to_peer_id());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn generating_over_an_existing_key_is_refused() {
        // Unrecoverable if it succeeded, so it must not be possible by accident.
        let dir = std::env::temp_dir().join(format!("cabal-relay-ow-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("relay.key");

        let first = generate(&path).unwrap();
        assert!(generate(&path).is_err());

        // And the original survived the attempt.
        assert_eq!(
            first.public().to_peer_id(),
            load(&path).unwrap().public().to_peer_id()
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn a_generated_key_is_not_world_readable() {
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!("cabal-relay-perm-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("relay.key");

        generate(&path).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "relay key must not be readable by other accounts");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_corrupt_key_is_malformed_rather_than_missing() {
        // The two need different responses: missing means "generate one and
        // back it up", malformed means "restore the backup".
        let dir = std::env::temp_dir().join(format!("cabal-relay-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("relay.key");
        std::fs::write(&path, b"not a keypair").unwrap();

        assert!(matches!(load(&path), Err(IdentityError::Malformed)));

        std::fs::remove_dir_all(&dir).ok();
    }
}
