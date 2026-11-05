# Tauri mobile plan verification — 2026-08-01

Scope: fact-check the updated architecture/UI claims against primary sources. This note does not replace either implementation plan.

## Verdict

The main corrections are valid: target Tauri 2.11 instead of a custom lifecycle plugin, require Xcode 26 for release, isolate platform capabilities, use absolute Vite outputs, give all streams explicit teardown, describe Cargo target dependencies as resolved-but-not-built for inactive targets, and make Android local-network access target-SDK-aware.

The fact-check found these refinements; all are now reflected in the two implementation plans:

1. Tauri 2.11.5 already documents the exact Rust variants and JavaScript event names; those are no longer unknown.
2. The iOS lifecycle pair is asymmetric for transient interruptions and must be device-tested before it controls mesh teardown.
3. The prior Vite correction still used `__dirname`, but this repository is ESM (`"type": "module"`), where `__dirname` is unavailable; the final sample is ESM-safe.
4. Export compliance must stay an unanswered release gate; neither `true` nor `false` should be inferred solely from “uses AES/Noise.”
5. Raw rust-libp2p mDNS needs Apple's multicast entitlement, and the pinned QUIC transport requires explicit disconnect/recovery rather than a path-migration promise.
6. Android 17 / target SDK 37+ blocks raw LAN and mDNS until `ACCESS_LOCAL_NETWORK` is granted; Android 16 offers an opt-in compatibility test with temporary `NEARBY_WIFI_DEVICES` instead.

## Claim verification

### 1. Tauri 2.11 mobile lifecycle — confirmed, with an iOS caveat

- [`tauri@2.11.0`](https://v2.tauri.app/release/tauri/v2.11.0/) added propagation of Tao mobile `Suspended` and `Resumed` events.
- In current Tauri 2.11.5, the public Rust path is documented as `RunEvent::WindowEvent { event: WindowEvent::Suspended | WindowEvent::Resumed, .. }`. The variants are mobile-only. Android maps them to Activity `onPause`/`onResume`; iOS maps them to `applicationWillResignActive`/`applicationWillEnterForeground`. See [`WindowEvent`](https://docs.rs/tauri/2.11.5/x86_64-apple-ios/tauri/enum.WindowEvent.html), [`RunEvent`](https://docs.rs/tauri/2.11.5/x86_64-apple-ios/tauri/enum.RunEvent.html), and the [Tauri source mapping](https://docs.rs/tauri/2.11.5/x86_64-apple-ios/src/tauri/app.rs.html).
- Tauri also emits the window events `tauri://suspended` and `tauri://resumed`; the JavaScript enum names are `TauriEvent.WINDOW_SUSPENDED` and `TauriEvent.WINDOW_RESUMED`. See the [window-manager source](https://docs.rs/tauri/2.11.5/src/tauri/manager/window.rs.html) and [JavaScript event API](https://v2.tauri.app/reference/javascript/api/namespaceevent/).
- The local Tauri 2.9.5 source has no `WindowEvent::Suspended/Resumed`. It does contain a top-level `RunEvent::Resumed`, but the local 2.9 runtime maps that from event-loop polling, not a paired mobile app lifecycle. Therefore “2.9.5 did not expose a usable suspend/resume pair” is precise; “gave nothing” is rhetorically broader than the API evidence.

#### Material iOS nuance

Apple says `willResignActive` occurs whenever the app loses active focus, including an overlay or device lock, while `willEnterForeground` occurs only when leaving the background. See [`willResignActiveNotification`](https://developer.apple.com/documentation/uikit/uiapplication/willresignactivenotification), [`willEnterForegroundNotification`](https://developer.apple.com/documentation/uikit/uiapplication/willenterforegroundnotification), and [`didBecomeActiveNotification`](https://developer.apple.com/documentation/uikit/uiapplication/didbecomeactivenotification).

Consequently, a transient overlay can theoretically produce Tauri `Suspended` without the matching Tauri `Resumed`: UIKit returns from inactive to active via `didBecomeActive`, not by leaving background. Before `Suspended` cancels subscriptions or pauses the mesh, test these device sequences:

- Notification Center and Control Center open/dismiss.
- Incoming-call interruption.
- Lock/unlock.
- Home/app-switcher background and foreground.

If events are unbalanced, use a narrow iOS bridge with a symmetric pair: `didEnterBackground`/`willEnterForeground` for true backgrounding, or `willResignActive`/`didBecomeActive` if inactive status is intentionally the policy.

### 2. Capabilities, core permissions, and AppManifest — confirmed

- Tauri [automatically enables every capability file](https://v2.tauri.app/security/capabilities/) unless `app.security.capabilities` explicitly lists identifiers. A window/webview in multiple capabilities receives their permission union. Therefore an unrestricted `default.json` can leak `opener:default` into mobile.
- Once identifiers are selected explicitly, unlisted files are not built into that configuration. Deleting `default.json` is still the clearest defensive cleanup, but explicit selection is the runtime control.
- [`core:default`](https://v2.tauri.app/reference/acl/core-permissions/) already includes app, event, image, menu, path, resources, tray, webview, and window defaults. Adding event/path beside it is redundant. Genuine least privilege should grant only the frontend calls observed in the final bundle. The final mobile plan uses `core:event:allow-listen` + `allow-unlisten`; `core:event:default` would also grant frontend emit operations that the UI does not use.
- [`AppManifest::commands`](https://docs.rs/tauri-build/2.6.3/tauri_build/struct.AppManifest.html) generates `allow-*` and `deny-*` permissions. A permission only affects a window when a [capability references it](https://v2.tauri.app/security/permissions/). Tauri's [IPC authorization source](https://docs.rs/tauri/2.11.5/src/tauri/webview/mod.rs.html) checks every local app command when an app ACL manifest exists and rejects calls with no resolved ACL. Thus introducing a manifest for only the future surface while today's 47 commands are live can lock desktop IPC; inventory/grant the current surface first. The final plan counts the future surface exactly: 28 handlers, with only 25 granted to the mobile webview.
- The `$schema` field provides generated schema/autocomplete support. It is not itself the runtime security boundary. Saying a desktop schema means mobile permissions “cannot be expressed at all” is too strong; platform files and the correct generated schema are still the right maintainable design.

### 3. Rust-only mobile plugins should receive no webview permission — confirmed

Tauri's [mobile plugin guide](https://v2.tauri.app/develop/plugins/develop-mobile/) says annotated Kotlin/Swift mobile commands can be called from Rust or JavaScript, while Rust uses `PluginHandle`/`run_mobile_plugin`. Tauri [permissions](https://v2.tauri.app/security/permissions/) enable exposed commands for a frontend window/webview.

Therefore:

- `keystore` and `multicast-lock`, if called only through Rust handles, should not be granted to the webview.
- Granting `allow-unwrap-key` to the main webview would authorize frontend script to reach that exposed command; this is a high-impact overgrant.
- The type-scale contract needed an ownership decision. The applied plan now documents two deliberate callers: JavaScript invokes `plugin:type-scale|get_scale` once at boot (so it retains `type-scale:allow-get-scale`), while Rust rereads via its plugin handle on resume and emits `TypeScaleChanged`. Keystore and multicast-lock remain Rust-only with no webview grant.

### 4. Vite output and Tauri dev hooks — confirmed; final sample is ESM-safe

- Vite defines [`root`](https://vite.dev/config/shared-options.html#root) as the project root containing `index.html` and defines [`build.outDir`](https://vite.dev/config/build-options.html#build-outdir) relative to that root. Thus `root: "src/mobile-entry"` plus `outDir: "dist-mobile"` writes under `src/mobile-entry/dist-mobile`.
- Tauri's [Vite guide](https://v2.tauri.app/start/frontend/vite/) configures both `beforeDevCommand`/`devUrl` and `beforeBuildCommand`/`frontendDist`. Platform configs are merged with the base config; the official [configuration guide](https://v2.tauri.app/develop/configuration-files/) specifies JSON Merge Patch semantics. Overriding only the build hook leaves mobile dev running the desktop Vite command.
- The pre-third-pass plan's absolute-output direction was correct, but its sample used `__dirname` even though this repository's `package.json` declares `"type": "module"`. Node's [ESM documentation](https://nodejs.org/api/esm.html#differences-between-es-modules-and-commonjs) states that `__dirname` is not available in ES modules. The applied plan now uses an ESM-safe directory value:

  ```ts
  import { resolve } from "node:path";
  import { fileURLToPath } from "node:url";

  const projectRoot = fileURLToPath(new URL(".", import.meta.url));
  const root = mobile
    ? resolve(projectRoot, "src/mobile-entry")
    : projectRoot;
  const outDir = resolve(projectRoot, mobile ? "dist-mobile" : "dist-desktop");
  ```

### 5. Apple release tooling — confirmed; this host definitely needs a macOS upgrade

- Apple states that since 2026-04-28, App Store Connect uploads must use [Xcode 26 or later and the iOS 26 SDK or later](https://developer.apple.com/news/upcoming-requirements/?id=02032026a).
- Apple lists Xcode 26 as requiring [macOS Sequoia 15.6 or later](https://developer.apple.com/support/xcode). The verified local host is macOS 14.6.1 with Xcode 15.4/iOS 17.5 SDK. Therefore this is not merely a possible macOS-floor increase: this machine must upgrade macOS before it can install Xcode 26 and produce an uploadable build.
- Xcode 15.4 remains usable for local iOS 17.5 simulator work and an early Rust/native dependency probe, but it cannot test the iOS 26 SDK build or ship to App Store Connect.

### 6. Export compliance — release gate confirmed; avoid pre-classification

Apple defines [`ITSAppUsesNonExemptEncryption`](https://developer.apple.com/documentation/bundleresources/information-property-list/itsappusesnonexemptencryption) as `NO` for no encryption or exempt encryption and `YES` for non-exempt encryption. With `YES`, Apple says an `ITSEncryptionExportComplianceCode` is typically also supplied; App Store builds flagged `YES` must be associated with an encryption declaration before beta review.

Apple's [export-compliance overview](https://developer.apple.com/help/app-store-connect/manage-app-information/overview-of-export-compliance) directs developers to answer the App Store Connect questionnaire. Its [documentation table](https://developer.apple.com/help/app-store-connect/reference/export-compliance-documentation-for-encryption/) also notes that an industry-standard algorithm implemented outside Apple's OS may require a French encryption declaration when distributing in France.

Therefore the correct plan is: questionnaire/classification first, then set the plist value and compliance code if Apple requires it. “Uses standard AES/Noise” alone proves neither exemption nor non-exemption, and “`true` always obliges a code” should be softened to Apple's “typically.”

### 7. Cargo target-specific resolution — confirmed

The Cargo Book says [platform-specific dependencies are resolved as if all platforms are enabled](https://doc.rust-lang.org/cargo/reference/resolver.html#dependency-kinds), and the resolution is stored in `Cargo.lock`. Target-specific dependency declarations remain the correct way to keep desktop-only crates from being compiled/linked into a mobile target. The precise wording is:

> Target-specific dependencies participate in dependency resolution and the lockfile, but inactive-target dependencies/features are not compiled or linked for the current build.

See also Cargo's [target-specific dependency syntax](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html#platform-specific-dependencies).

### 8. iOS multicast and libp2p QUIC — confirmed, with stronger constraints

#### 8.1 `rust-libp2p` mDNS needs the restricted iOS multicast entitlement

Apple treats three controls as distinct:

- `NSLocalNetworkUsageDescription` explains the local-network access request.
- `NSBonjourServices` declares the Bonjour service types used by the app.
- [`com.apple.developer.networking.multicast`](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.developer.networking.multicast) authorizes direct IP multicast/broadcast on iOS and requires Apple approval.

Apple's [local-network privacy technote](https://developer.apple.com/documentation/technotes/tn3179-understanding-local-network-privacy) classifies sending or receiving UDP multicast as requiring the multicast entitlement. Apple's [Bonjour/local-network guidance](https://developer.apple.com/videos/play/wwdc2020/10110/) separately explains that ordinary Bonjour browse/advertise operations declare their service types in `Info.plist`, while apps doing multicast discovery without Bonjour need the entitlement. Therefore the usage string and Bonjour declaration do not authorize a raw multicast socket by themselves.

This matters because the currently locked `libp2p-mdns` 0.46.0 is not merely delegating discovery to Apple's Bonjour API: its [implementation](https://docs.rs/crate/libp2p-mdns/0.46.0/source/src/behaviour/iface.rs) joins the mDNS multicast group and sends UDP traffic to port 5353; the protocol module uses `_p2p._udp.local`. For iOS there are three realistic choices:

1. Request Apple's multicast entitlement and keep `rust-libp2p` mDNS. Treat approval and the entitlement-bearing provisioning profile as release gates.
2. Build a narrow native Bonjour bridge with `NWBrowser`/`NWListener` or `NetService`, declare `_p2p._udp`, and pass discovered peer IDs/multiaddrs into the Rust swarm. Prototype record compatibility first: system Bonjour is not automatically a drop-in implementation of the `libp2p-mdns` wire contract.
3. Disable LAN mDNS on iOS and rely on bootstrap/relay/rendezvous or explicit pairing. This is the predictable fallback if Apple declines the entitlement or the native bridge is not worth its maintenance cost.

`NSBonjourServices` may still be appropriate for a native Bonjour implementation, but it must not be presented as a substitute for the multicast entitlement when the app uses `libp2p-mdns`'s raw UDP sockets.

TN3179 also states that there is **no general API** returning whether the process currently has Local Network access. Specific Bonjour/Network.framework operations can surface policy-denied states, but a raw UDP listener that receives nothing cannot distinguish denial from a network with zero peers. The runtime contract therefore needs an `indeterminate` discovery state rather than a confident `mdns_granted: bool`; resume reruns the probe but does not manufacture certainty.

#### 8.2 libp2p QUIC does not use Noise/Yamux, and seamless path migration is unavailable today

The official [libp2p QUIC transport documentation](https://github.com/libp2p/docs/blob/master/content/concepts/transports/quic.md) describes QUIC as UDP transport, TLS 1.3 security, and native stream multiplexing in one protocol. The Rust implementation likewise states that QUIC provides transport, security, and multiplexing without the upgrade phase used by other libp2p transports. Consequently:

- QUIC connections do **not** layer Noise or Yamux on top.
- A TCP fallback can still use Noise (or TLS) plus Yamux, but that is a separate transport stack.

The migration claim needs a stronger correction than “device-test it.” Cabal Mesh currently resolves `libp2p-quic` 0.11.1, whose configuration disables connection migration. The [current upstream Rust implementation](https://github.com/libp2p/rust-libp2p/blob/master/transports/quic/src/config.rs) still calls `server_config.migration(false)` and says migration should only be enabled once local-address changes can be handled correctly. Therefore the architecture must not promise that an established libp2p QUIC connection survives a Wi-Fi-to-cellular handoff.

Design for disconnect and recovery instead: detect the lost connection/path change, redial bootstrap peers, restore pubsub membership and other subscriptions, and make replay/deduplication explicit. Device tests should measure that recovery path—including interruption length, duplicate events, and settlement continuity—not treat seamless QUIC migration as the expected result. If a future transport version or custom integration enables endpoint rebinding and migration, re-validate that behavior on real iOS and Android devices before changing the guarantee.

### 9. Exact Tauri release set — independently versioned packages

The [official release index](https://v2.tauri.app/release/) on 2026-08-01 gives the compatible set selected by the plan: Rust `tauri` 2.11.5, `tauri-build` 2.6.3, JavaScript API 2.11.1, npm CLI 2.11.4, and opener 2.5.4. The [dependency-update guide](https://v2.tauri.app/develop/updating-dependencies/) requires Tauri core and JavaScript API to share the 2.11 minor, while each official plugin's Rust and npm packages must match exactly. Build and plugin crates version independently, so wording like “move every package to 2.11” is wrong.

The implementation manifests use exact constraints: Cargo leading `=` (`=2.11.5`, `=2.6.3`, `=2.5.4`) and npm bare versions (no `^`/`~`). The npm plugin half should be removed if the frontend has no corresponding JavaScript import.

### 10. Android 16/17 local-network protection — target-dependent permission confirmed

Android's [Local Network Protection guide](https://developer.android.com/privacy-and-security/local-network-permission), updated 2026-07-13, explicitly covers raw sockets, mDNS, TCP and UDP multicast/broadcast:

- On Android 16 / target SDK 36, enforcement is opt-in through the `RESTRICT_LOCAL_NETWORK` compatibility flag. The temporary test permission is `NEARBY_WIFI_DEVICES`; ordinary apps targeting SDK 36 or lower continue to receive LAN access through `INTERNET` and must **not** declare/request `ACCESS_LOCAL_NETWORK`.
- Starting with Android 17, apps targeting SDK 37+ are blocked from LAN traffic by default. Broad raw-socket use must declare and request the runtime `ACCESS_LOCAL_NETWORK` permission, then handle denial and revocation. UDP denial typically surfaces as `EPERM`.
- A system-mediated `NsdManager` picker can authorize a user-selected service without the broad permission, but CabalMesh's current rust-libp2p swarm performs broad raw multicast discovery. Treating the picker as equivalent would require a native discovery redesign and libp2p wire-compatibility proof.

Tauri's official [`AndroidConfig`](https://v2.tauri.app/reference/config/#androidconfig) exposes `minSdkVersion` but no target-SDK field. Consequently the generated `src-tauri/gen/android/app/build.gradle.kts` target must be pinned/reviewed after `android init`; otherwise the permission contract can change when the Gradle/toolchain target changes.

The [`WifiManager.MulticastLock` contract](https://developer.android.com/reference/android/net/wifi/WifiManager.MulticastLock) still states that the Wi-Fi stack normally filters multicast packets and that the lock enables their reception at a battery cost. Newer [`NsdManager` guidance](https://developer.android.com/reference/android/net/nsd/NsdManager) says the system automatically manages foreground multicast reception from T-extension 7 onward, but that guarantee is documented for `NsdManager`, not rust-libp2p's raw UDP sockets. Keep the narrowly held lock until physical-device tests prove it unnecessary for this transport.
