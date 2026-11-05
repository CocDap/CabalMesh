//! The identifiers every node must agree on.
//!
//! # Why these live with the protocol
//!
//! They are not a property of CoreBluetooth or of Android's BLE stack; they are
//! what makes two nodes members of the same mesh. A macOS radio and an Android
//! radio that disagree here are two meshes that cannot see each other, and the
//! symptom is silence rather than an error.
//!
//! So there is one definition, in the crate that has no platform in it, and
//! both radios read it.

/// The service every CabalMesh node advertises and scans for.
///
/// A UUID is **hexadecimal**. The first version of this constant spelled a word
/// containing an `H`, which is not a hex digit. `CBUUID::UUIDWithString` does
/// not return an error for that — it raises an Objective-C exception, which
/// killed the process from inside a delegate callback with a stack trace naming
/// KVO rather than this constant. Nothing in the type system prevented it,
/// which is why [`is_valid_uuid`] exists and is tested.
pub const SERVICE_UUID: &str = "CABA1E5E-0000-4000-8000-CABA1E5E1001";

/// The characteristic carrying a node's L2CAP PSM.
///
/// Differs from [`SERVICE_UUID`] in one field, so the pair reads as a set.
pub const PSM_UUID: &str = "CABA1E5E-0001-4000-8000-CABA1E5E1001";

/// Whether a string is a UUID both platforms will accept.
///
/// 8-4-4-4-12 hexadecimal digits. Checked rather than trusted, because one
/// platform's failure mode is a process-killing exception rather than an error
/// value.
#[must_use]
pub fn is_valid_uuid(value: &str) -> bool {
    const WIDTHS: [usize; 5] = [8, 4, 4, 4, 12];

    let groups: Vec<&str> = value.split('-').collect();
    groups.len() == WIDTHS.len()
        && groups.iter().zip(WIDTHS).all(|(group, width)| {
            group.len() == width && group.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_identifiers_are_valid_uuids() {
        // The regression: a single non-hex character aborted the process from
        // inside a CoreBluetooth callback, and it took a run against real
        // hardware to find.
        assert!(is_valid_uuid(SERVICE_UUID), "{SERVICE_UUID} is not a UUID");
        assert!(is_valid_uuid(PSM_UUID), "{PSM_UUID} is not a UUID");
    }

    #[test]
    fn the_service_and_the_characteristic_differ() {
        // One UUID for both is a characteristic whose type matches its
        // service, which discovery cannot then tell apart.
        assert_ne!(SERVICE_UUID, PSM_UUID);
    }

    #[test]
    fn a_non_hex_character_is_refused() {
        assert!(!is_valid_uuid("C4B41E5H-0000-4000-8000-CABA1E5E5401"));
        assert!(!is_valid_uuid("CABA1E5E-0000-4000-8000-CABA1E5E100Z"));
    }

    #[test]
    fn wrong_group_widths_are_refused() {
        assert!(!is_valid_uuid("CABA1E5-0000-4000-8000-CABA1E5E1001"));
        assert!(!is_valid_uuid("CABA1E5E-000-4000-8000-CABA1E5E1001"));
        assert!(!is_valid_uuid("CABA1E5E-0000-4000-8000-CABA1E5E100"));
        assert!(!is_valid_uuid("CABA1E5E000040008000CABA1E5E1001"));
        assert!(!is_valid_uuid(""));
    }

    #[test]
    fn case_does_not_matter_to_the_check() {
        // Android's `UUID.fromString` is case-insensitive and CoreBluetooth
        // normalises; a validator stricter than both would reject a UUID that
        // works.
        assert!(is_valid_uuid(&SERVICE_UUID.to_lowercase()));
    }
}
