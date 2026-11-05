# iOS release

**Ticket 37.** Xcode, signing, the encryption declaration, size, and the upload.

Configuration is committed. The steps needing an Apple account, a newer Xcode, or a BIS filing are marked — they are the only ones outstanding, and two of them have lead time measured in weeks rather than minutes.

---

## The installed Xcode cannot ship

Since **28 April 2026**, App Store Connect rejects any build not made with **Xcode 26 or later against a version-26 SDK**. This machine has **15.4**.

That is fine for simulator work and for the cross-compile probe in `docs/mobile-build-verification.md`, and no build from it is uploadable. The upgrade may also raise the macOS floor, so check that before setting aside an afternoon for it.

### One thing to know before upgrading

**Do not run `xcodegen generate` with the currently installed xcodegen until Xcode is upgraded.** It rewrites `project.pbxproj` with `objectVersion = 77` — Xcode 16's format — which Xcode 15.4 then cannot open. Discovered while closing ticket 05; the change was reverted rather than committed.

After the upgrade this stops being a hazard and becomes a required step, because:

---

## The development team comes from the environment

`project.yml` carries:

```yaml
DEVELOPMENT_TEAM: ${APPLE_DEVELOPMENT_TEAM}
```

Xcode expands that from the process environment, so the whole configuration is:

```sh
export APPLE_DEVELOPMENT_TEAM=ABCDE12345
```

**No team identifier is in version control**, which is the ticket's requirement. A committed one leaks an organisation and pins every contributor's build to a single account. Unset, it yields an empty team and fails at signing with a message naming the variable — a better failure than silently signing with somebody else's.

> `project.yml` is the source of truth; `project.pbxproj` is generated from it. This setting reaches the build **only after the next `xcodegen generate`** — which, per the section above, must wait for the Xcode upgrade. Until then a device build still needs the team set in Xcode's UI.

---

## Encryption declaration

`ITSAppUsesNonExemptEncryption` is **`true`**, in `project.yml` as well as the plist so regeneration cannot drop it.

The determination is in **`docs/export-compliance.md`**, and it did not come back the way the ticket expected. The app ships its own AES-256-GCM vault and Noise transport in Rust rather than calling only the OS, so it clears none of the four questionnaire exemptions. Classification is **5D992.c** — mass market, self-classifiable.

That obliges two filings, and both block the first upload:

1. **An ERN** from BIS, via an encryption registration in SNAP-R.
2. **An annual self-classification report** to BIS and the NSA, due 1 February each year for the preceding year. Recurring, not one-off.

`ITSEncryptionExportComplianceCode` is deliberately **absent**. Apple issues it only once export documentation is accepted; guessing it or omitting-then-inventing it is precisely what ticket 05 exists to prevent.

**Start these before the build is ready.** They are the long pole and nothing about them is faster for being left late.

---

## The multicast entitlement is a separate approval

`com.apple.developer.networking.multicast` is in the entitlements because libp2p's mDNS sends raw UDP multicast, which iOS 14+ blocks without it.

Apple grants it only through the [Multicast Networking Entitlement request form](https://developer.apple.com/contact/request/networking-multicast), per bundle identifier. **Until it is approved for `com.cabalmesh.app`, signing a device build with it present fails.** Another item with review latency — request it alongside the export filings.

---

## Size

```sh
export APPLE_DEVELOPMENT_TEAM=ABCDE12345
npm run ios:size
```

Builds a device release archive and reports the archive size, the `.app` payload, the main binary, its architecture slices, and the ten largest files.

These are **pre-thinning** figures. The number a user sees is the thinned, re-signed, DRM-wrapped payload for one device family, and only App Store Connect produces it — it will be smaller than anything measured here. The architecture listing is there to catch a build that still carries a simulator slice, which inflates every other figure.

---

## Upload

```sh
xcrun altool --upload-app -f build/CabalMesh.ipa -t ios \
  --apiKey "$ASC_KEY_ID" --apiIssuer "$ASC_ISSUER_ID"
```

An App Store Connect API key beats an Apple ID here: it is scoped, revocable, and does not need an app-specific password rotated by hand.

---

## Still to do

- [ ] Xcode upgraded to 26 or later; check the macOS floor first
- [ ] `xcodegen generate` re-run **after** that upgrade, so `DEVELOPMENT_TEAM` reaches the project
- [ ] Multicast networking entitlement requested and approved for `com.cabalmesh.app`
- [ ] ERN obtained and the annual self-classification report scheduled
- [ ] `ITSEncryptionExportComplianceCode` set once Apple issues it
- [ ] Release build succeeds against the version-26 SDK
- [ ] Size measured and recorded
- [ ] Archive uploaded

Three of these — the entitlement, the ERN, the compliance code — are other people's queues. Everything else is an afternoon once Xcode is current.
