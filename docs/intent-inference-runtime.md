# Local intent inference runtime decision

**Status:** accepted for the always-available v1 path, 2026-08-12

This decision implements the boundary in
[`intent-chat-and-modules-design.md`](intent-chat-and-modules-design.md): the
model may propose fields, but Rust validates them and only the user can approve
a broadcast. It does not make conversational input an execution command.

## Decision

CabalMesh uses `cabal-intent-slots-v1`, a compact deterministic semantic-slot
model implemented by the pure Rust `cabal-intent-inference` crate. The app
depends on that crate and successfully compiles it for desktop, iOS, and
Android. The `propose_intent` command now calls that API in a bounded local
worker and returns only structured chips; it never returns or logs the phrase.
There is no model download, hosted endpoint, local-network server, platform AI
entitlement, or operator configuration in the required path.

The v1 model proposes exactly six typed fields:

- action: buy, sell, swap, or stake;
- asset: wNEAR or USDC;
- amount, parsed with the selected asset's precision;
- condition: below a USD price, above a USD price, or any/market price;
- execution mode: shark, ghost, or patient;
- privacy: low, medium, or high.

Missing fields remain `None`. Conflicting signals, malformed values, control
characters, overlong text, and known instruction-manipulation phrases return a
typed error. Inference has no I/O dependency and has no function that creates,
signs, queues, or broadcasts an intent. The application passes every complete
proposal through the same `parse_draft` function used by preview and broadcast
before it becomes reviewable. Review and explicit confirmation remain separate
commands.

## Packaging and fallback

The model's signal tables and parser are an ordinary Rust application
dependency, so referenced code is linked into each target with no runtime
loader. The measured table footprint is 1,184 bytes, and the largest measured
standalone probe executable is 526,320 bytes. No asset needs to be copied into
an application bundle or updated out of band.

Fallback is fail-closed:

1. A safe partial parse becomes a clarification request for its missing fields.
2. Ambiguous, malformed, or adversarial input becomes a correction prompt with
   no candidate draft.
3. An unsupported action or asset remains absent and therefore cannot reach
   review or broadcast.
4. If the inference crate is unavailable or returns an error, the editable
   typed form remains the manual path; there is no remote inference fallback.

System or bundled generative models may later improve language coverage, but
their output must enter this same typed proposal boundary and may never become
the availability or safety gate.

## Target proof

The release-mode probe parses one complete buy, sell, swap, and stake phrase at
startup, then parses those phrases 100,000 times through the public inference
boundary. Resident memory is the whole probe process, not an estimate of only
the model. Startup is entry-to-first four successful proposals. Measurements
were taken on 2026-08-12.

| Target class | Environment | Model | Executable | Startup | 100k elapsed | Mean/inference | Resident set |
|---|---|---:|---:|---:|---:|---:|---:|
| Desktop | Apple M1 Max, macOS 14.6.1, arm64 | 1,184 B | 513,376 B | 31 µs | 381 ms | 3,818 ns | 1,490,944 B peak |
| iOS | iPhone 15 Pro Simulator, iOS 17.5, arm64 | 1,184 B | 495,040 B | 350 µs | 330 ms | 3,305 ns | 8,929,280 B resident |
| Android | API 34 `sdk_gphone64_arm64`, arm64-v8a | 1,184 B | 526,320 B | 156 µs | 419 ms | 4,191 ns | 3,022,848 B resident |

The measured battery proxy is active CPU work per user-triggered inference:
3.3–4.2 microseconds on these target classes. The runtime performs no polling,
background inference, network request, or radio wake-up. Even an artificial 10
parses per minute for 24 hours extrapolates to about 0.05–0.06 seconds of this
measured active work. Simulators do not provide trustworthy physical battery
energy, so real-device energy profiling remains a release-quality check rather
than a reason to invent a watt-hour number here.

Reproduction commands:

```sh
cd src-tauri
cargo run -p cabal-intent-inference --example probe --release
cargo build -p cabal-intent-inference --example probe --release \
  --target aarch64-apple-ios-sim
xcrun simctl spawn booted \
  target/aarch64-apple-ios-sim/release/examples/probe

CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/darwin-x86_64/bin/aarch64-linux-android24-clang" \
  cargo build -p cabal-intent-inference --example probe --release \
  --target aarch64-linux-android
adb push target/aarch64-linux-android/release/examples/probe \
  /data/local/tmp/cabal-intent-probe
adb shell chmod 755 /data/local/tmp/cabal-intent-probe
adb shell /data/local/tmp/cabal-intent-probe
```

The proof suite also covers representative field values, absent fields,
conflicting actions, multiple assets, amount/price separation, unsupported
assets, over-precision values, instruction manipulation, debug redaction, the
model-size bound, and arbitrary UTF-8 input without panics.

## Rejected mandatory runtimes

- **Apple Foundation Models:** attractive and on-device, but availability
  depends on an eligible device, Apple Intelligence being enabled, and the
  system model being ready. It cannot be the universal iOS fallback. See
  [Apple Foundation Models](https://developer.apple.com/documentation/FoundationModels)
  and [availability guidance](https://developer.apple.com/documentation/FoundationModels/generating-content-and-performing-tasks-with-foundation-models).
- **Gemini Nano through Android AICore:** on-device and system-managed, but
  model and feature availability depend on the Android device. It is suitable
  only as an optional enhancement. See
  [Gemini Nano](https://developer.android.com/ai/gemini-nano).
- **Bundled llama.cpp plus GGUF:** portable across Android and Apple targets,
  but it adds a native runtime, a separately packaged model, materially larger
  storage and working memory, and more complex mobile builds for a six-slot
  constrained output. See the official
  [Android build guide](https://github.com/ggml-org/llama.cpp/blob/master/docs/android.md)
  and [iOS XCFramework example](https://github.com/ggml-org/llama.cpp/blob/master/examples/llama.swiftui/README.md).
- **Hosted model or operator-configured Ollama server:** broad language
  coverage does not outweigh transmitting private intent text, depending on a
  network, and making a second service part of compose availability. It is
  explicitly not a fallback for this feature.

The rejected options can be reconsidered for optional language coverage after
the typed safety boundary and target availability checks are already in place.
