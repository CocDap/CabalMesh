# Android release

**Ticket 38.** Release signing, the app bundle, per-device size, and the first Play upload.

Everything here that is code or configuration is committed. The three steps that need a keystore and a Play account are marked, and they are the only ones outstanding.

---

## The keystore

### Create it — once, ever

```sh
keytool -genkeypair -v \
  -keystore ~/keys/cabalmesh-release.jks \
  -alias cabalmesh \
  -keyalg RSA -keysize 4096 \
  -validity 10000 \
  -storetype PKCS12
```

**PKCS12, not JKS.** JKS is proprietary and `keytool` warns about it on every use. **4096-bit RSA**, and `-validity 10000` (~27 years) because Play requires a key valid well past any plausible release.

This key has the same character as the relay identity: **losing it means you can no longer update the app.** Not "it becomes awkward" — the listing is permanently frozen unless Play App Signing was enrolled with a key you can still produce an upload key for. Back it up somewhere that is not the machine that builds.

### Point the build at it

Create `src-tauri/gen/android/keystore.properties` — already in that directory's `.gitignore`, alongside `key.properties`:

```properties
storeFile=/Users/you/keys/cabalmesh-release.jks
storePassword=…
keyAlias=cabalmesh
keyPassword=…
```

Then `chmod 600` it. It holds two passwords in plaintext, which is the price of an unattended build; treat the file as the secret it is.

**Nothing here is committed and nothing falls back.** With the file absent the release build produces an **unsigned** APK rather than a debug-signed one. That is deliberate: a release APK signed with the debug key installs, runs, and looks entirely correct — and is rejected by Play with a message about the wrong certificate, at the point where the mistake is most expensive to discover.

---

## Building

### The bundle Play receives

```sh
npm run android:bundle
```

`.aab`, at `src-tauri/gen/android/app/build/outputs/bundle/universalRelease/`. Play splits it per device itself.

### Per-architecture APKs, for measuring

```sh
npm run android:size
```

Builds the four ABIs separately and prints each one's size. Use this and not the bundle to judge what a user downloads: the universal APK carries four copies of the Rust static library and is roughly four times what any device actually installs.

The budget question the ticket asks is answered against the **arm64-v8a** figure, because that is what every device shipped in the last several years installs. `armeabi-v7a` matters only for hardware old enough that `minSdk = 24` is the more pressing limit.

---

## Reproducibility

The signed bundle is reproducible in the sense that matters — the same source and the same keystore produce a bundle Play accepts as an update — but **not bit-identical**. Two things prevent that, and both are outside this repository:

- `versionCode` comes from `tauri.properties`, which is regenerated per build.
- The Rust toolchain embeds absolute paths unless `--remap-path-prefix` is set, which is not currently configured.

Worth stating rather than implying: nothing here verifies a byte-for-byte rebuild, so do not claim one.

---

## The first upload is manual, and has to be

Play verifies the signature and the bundle identifier against the account before any automation is possible. The first release goes through the console by hand:

1. Play Console → Create app → `com.cabalmesh.app`
2. **Enrol in Play App Signing.** Google holds the app signing key and you hold an upload key. Losing the upload key is then recoverable; losing the app signing key would not be. Do not skip this to save a step.
3. Upload the `.aab` to internal testing first, never straight to production.
4. Complete the Data safety form. Answer it against what the app actually does — see below.
5. Complete the US export declaration. `docs/export-compliance.md` is the determination it needs; the app is **not exempt**.

### Data safety, answered honestly

The form asks what is collected and shared. The truthful answers for this app:

- **No data is collected** by the developer. There is no analytics, no crash reporting, no account.
- **The relay observes connection metadata** — peer identifiers, IP addresses, who is connected to whom. `docs/relay-operations.md` documents this in full. Whether Play counts a self-hosted relay as "collection" depends on retention; with logging at the shipped default nothing is retained, and the form should be answered against whatever the deployed relay actually does rather than against the default.
- **Data is encrypted in transit** — Noise on every mesh hop, TLS to the chain RPC.
- **Data is encrypted at rest** — AES-256-GCM over the key vault.

---

## Still to do

- [ ] Release keystore created and backed up outside version control
- [ ] `keystore.properties` written on the build machine, `chmod 600`
- [ ] Signed bundle built and per-ABI sizes measured against the budget
- [ ] First upload completed manually in the console, with Play App Signing enrolled
- [ ] Signature and bundle identifier verified by Play

The first two need a keystore, the last three need a Play account. The build configuration they run against is committed and does not need revisiting.
