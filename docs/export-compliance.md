# Export-compliance determination

**Ticket 05.** Worked 2026-08-03 against the App Store Connect export-compliance questionnaire and the cryptography the app actually ships.

> **This is an engineering determination, not legal advice.** It records what the app does, which questionnaire branch that lands on, and why — so that whoever signs the declaration is signing something researched rather than guessed. **It needs counsel or a compliance owner's sign-off before the first upload.** A wrong self-declaration is a compliance problem, and the person who clicks the button owns it.

## Determination

**Non-exempt.** `ITSAppUsesNonExemptEncryption` is set to `true`.

Classification is **ECCN 5D992.c** — mass-market encryption software, self-classifiable. Not 5D002: the app is publicly available through a retail app store, its cryptographic functionality cannot be modified by the user, and it installs without supplier support, which is Note 3 to Category 5 Part 2.

## Why not exempt

The questionnaire offers a narrow set of exemptions. The app clears none of them, and the reasoning matters more than the answer:

| Exemption | Applies? | Why |
|---|---|---|
| Encryption is limited to what the operating system provides | **No** | The app carries its own implementations in Rust — `aes-gcm`, `rustls`, `snow`. It calls Apple's crypto for nothing load-bearing. |
| Limited to authentication, digital signature, or decrypting copy-protected data | **No** | Ed25519 transaction signing would qualify on its own, but the vault encrypts private keys at rest and the Noise transport encrypts peer traffic. Both are data confidentiality, which is outside the carve-out. |
| Keys ≤ 56-bit symmetric, ≤ 512-bit asymmetric, ≤ 112-bit elliptic curve | **No** | AES-256 and X25519 are both above every one of those thresholds. |
| Not designed with cryptographic functionality | **No** | Encryption is the product thesis, not an incidental feature. |

The ticket's caution — that most apps using standard cryptography for standard purposes qualify for an exemption — is correct as a general matter and does not hold here. What usually qualifies an app is that its only encryption is HTTPS provided by the OS. This app encrypts a key vault at rest and a peer-to-peer transport, using implementations it ships itself.

## What the app actually does

Every algorithm is standard and published. Nothing is proprietary, and nothing is a modified or reduced-strength variant.

| Purpose | Crate | Algorithm |
|---|---|---|
| Vault at rest | `aes-gcm` 0.10 | AES-256-GCM |
| Vault key derivation | `argon2` 0.5 | Argon2id |
| Peer transport | `libp2p-noise` 0.45, `snow` 0.9 | Noise XX — X25519, ChaCha20-Poly1305 |
| Peer identity | `ed25519-dalek` 2.2, `x25519-dalek` 2.0 | Ed25519, X25519 |
| HTTPS to the chain RPC | `rustls` 0.23, `aws-lc-rs`, `ring` | TLS 1.3 |
| Transaction signing | `ed25519-dalek` 2.2 | Ed25519 |
| Hashing | `sha2` 0.10 / 0.11 | SHA-256 |
| Memory hygiene | `zeroize` 1.8 | — |

**There is no zero-knowledge proving in any build.** The Noir circuit and its `nargo` shell-out were removed on 2026-08-27 (nothing called them), so no proving code ships on any platform and none is in scope for this determination.

## What must happen before the first upload

Neither of these can be done from a development machine, and both block ticket 37.

1. **Encryption registration → ERN.** File an encryption registration in BIS's SNAP-R and receive an Encryption Registration Number. Self-classification under 5D992.c requires it before export.
2. **Annual self-classification report.** Due to BIS and the NSA by **1 February** each year, covering the prior calendar year. This is recurring, not one-off — it outlives the release that triggers it.

`ITSEncryptionExportComplianceCode` is **deliberately not set.** Apple issues that code only after export documentation is accepted, and inventing or omitting-then-guessing it is exactly the failure mode this ticket exists to avoid. Set it when Apple issues it.

France's ANSSI declaration is covered by Apple's own filing for App Store distribution and needs nothing here.

## Android

Play has no manifest equivalent. The US export-law acknowledgment is a console checkbox at first release, and the determination above is what answers it.
