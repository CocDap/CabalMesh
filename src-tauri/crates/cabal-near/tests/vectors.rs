//! Golden vectors captured from the live Tidepool verifier by
//! `scripts/near-intents-spike/probe.mjs`. See `docs/near-intents-spike.md`.

use cabal_near::{
    sign_function_calls, FunctionCall, GAS_EXECUTE_INTENTS, GAS_FT_TRANSFER_CALL,
    token_diff_payload, verify, versioned_nonce, AccountId, MultiPayload, NearKeypair, TokenDiff,
};
use serde_json::Value;

fn fixture(name: &str) -> Value {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn hex32(value: &Value) -> [u8; 32] {
    hex::decode(value.as_str().unwrap()).unwrap().try_into().unwrap()
}

fn keypair_of(f: &Value) -> NearKeypair {
    NearKeypair::from_secret_bytes(&hex32(&f["secret_hex"]))
}

fn diff_of(f: &Value) -> TokenDiff {
    TokenDiff::from_iter(
        f["diff"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(token, amount)| (token.clone(), amount.as_str().unwrap().parse::<i128>().unwrap())),
    )
}

fn fixture_payload() -> MultiPayload {
    let f = fixture("nep413_token_diff.json");
    let verifier = AccountId::try_from(f["verifier"].as_str().unwrap()).unwrap();
    token_diff_payload(
        &keypair_of(&f),
        &verifier,
        &diff_of(&f),
        f["deadline_ms"].as_u64().unwrap(),
        hex32(&f["nonce_hex"]),
    )
}

#[test]
fn implicit_account_matches_fixture() {
    let f = fixture("keypair.json");
    let kp = keypair_of(&f);
    assert_eq!(kp.account_id().as_str(), f["account_id"].as_str().unwrap());
    assert_eq!(kp.public_key_string(), f["public_key"].as_str().unwrap());
    assert_eq!(kp.secret_bytes(), hex32(&f["secret_hex"]));
}

#[test]
fn debug_never_prints_the_secret() {
    let f = fixture("keypair.json");
    let printed = format!("{:?}", keypair_of(&f));
    assert!(printed.contains(f["account_id"].as_str().unwrap()));
    assert!(!printed.contains(f["secret_hex"].as_str().unwrap()));
}

#[test]
fn generated_keys_differ() {
    assert_ne!(NearKeypair::generate().secret_bytes(), NearKeypair::generate().secret_bytes());
}

#[test]
fn token_diff_payload_matches_fixture_byte_for_byte() {
    let f = fixture("nep413_token_diff.json");
    let p = fixture_payload();
    assert_eq!(serde_json::to_value(&p).unwrap(), f["expected_multipayload"]);
    assert_eq!(hex::encode(p.hash().unwrap()), f["expected_hash_hex"].as_str().unwrap());
    assert!(verify(&p));
}

#[test]
fn tampered_diff_fails_verification() {
    let mut p = fixture_payload();
    p.payload.message = p.payload.message.replace("4040000", "4040001");
    assert!(!verify(&p));
}

#[test]
fn a_key_swapped_in_after_signing_fails_verification() {
    let mut p = fixture_payload();
    p.public_key = NearKeypair::generate().public_key_string();
    assert!(!verify(&p));
}

#[test]
fn malformed_payload_is_refused_not_panicked_on() {
    let mut p = fixture_payload();
    p.signature = "ed25519:!!".into();
    assert!(!verify(&p));

    let mut p = fixture_payload();
    p.public_key = "secp256k1:abc".into();
    assert!(!verify(&p));

    let mut p = fixture_payload();
    p.payload.nonce = "not base64".into();
    assert!(!verify(&p));
}

#[test]
fn signer_is_read_back_from_the_signed_message() {
    let f = fixture("keypair.json");
    let p = fixture_payload();
    assert_eq!(p.signer_id().unwrap(), f["account_id"].as_str().unwrap());
}

#[test]
fn versioned_nonce_layout() {
    let n = versioned_nonce([0xfc, 0x4d, 0x10, 0xf1], 1_000, [7; 15]);
    assert_eq!(&n[0..5], &[0x56, 0x28, 0xf6, 0xc6, 0x00]);
    assert_eq!(&n[5..9], &[0xfc, 0x4d, 0x10, 0xf1]);
    assert_eq!(i64::from_le_bytes(n[9..17].try_into().unwrap()), 1_000_000_000);
    assert_eq!(&n[17..], &[7; 15]);
}

#[test]
fn versioned_nonce_matches_fixture() {
    let f = fixture("nep413_token_diff.json");
    let salt: [u8; 4] = hex::decode(f["salt_hex"].as_str().unwrap()).unwrap().try_into().unwrap();
    let tail: [u8; 15] = hex::decode(f["nonce_tail_hex"].as_str().unwrap()).unwrap().try_into().unwrap();
    assert_eq!(versioned_nonce(salt, f["deadline_ms"].as_u64().unwrap(), tail), hex32(&f["nonce_hex"]));
}

#[test]
fn account_id_rules() {
    for ok in ["ab", "verifier.tidepool-sandbox.testnet", "a_b-c.near"] {
        assert!(AccountId::try_from(ok).is_ok(), "{ok}");
    }
    for bad in ["", "a", "UPPER.near", "a..b", ".a", "a.", "-a", "a b", &"x".repeat(65)] {
        assert!(AccountId::try_from(bad).is_err(), "{bad}");
    }
}

#[test]
fn signed_ft_transfer_call_matches_fixture() {
    let f = fixture("signed_tx_ft_transfer_call.json");
    let block_hash: [u8; 32] =
        bs58::decode(f["block_hash"].as_str().unwrap()).into_vec().unwrap().try_into().unwrap();
    let call = FunctionCall {
        method_name: f["method_name"].as_str().unwrap().to_owned(),
        args: f["args_json"].as_str().unwrap().as_bytes().to_vec(),
        gas: f["gas"].as_str().unwrap().parse().unwrap(),
        deposit: f["deposit"].as_str().unwrap().parse().unwrap(),
    };
    let receiver = AccountId::try_from(f["receiver_id"].as_str().unwrap()).unwrap();
    let signed = sign_function_calls(&keypair_of(&f), &receiver, f["nonce"].as_u64().unwrap(), block_hash, &[call]);
    assert_eq!(signed, f["expected_signed_tx_base64"].as_str().unwrap());
}

#[test]
fn gas_constants_match_the_spike() {
    assert_eq!(GAS_FT_TRANSFER_CALL, 100_000_000_000_000);
    assert_eq!(GAS_EXECUTE_INTENTS, 300_000_000_000_000);
}
