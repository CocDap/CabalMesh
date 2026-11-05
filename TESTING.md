# 🧪 CabalMesh - Testing Guide

Follow this guide to verify all 4 layers of the CabalMesh privacy stack (see
the Privacy Layers table in [README.md](README.md)): mesh networking, AI
negotiation and settlement on NEAR Intents.

## 🚀 1. Start the Application

```bash
npm run tauri dev
```

Wait for `✅ System Bootstrap Complete. Mesh Swarm Active.` in the terminal —
the app then opens on the Home screen.

---

## 🔒 2. Test Intent Broadcast

**Action:**
1. From Home, open **New** and compose an intent, e.g.:
   ```
   Buy 1 wNEAR under $4.20 using Shark Mode
   ```
2. Broadcast it. This takes you to the **Detail** screen.

**Verify in UI (Detail):**
- The status badge moves from `BROADCAST` toward `NEGOTIATING`.
- The **VERIFICATION LOG** panel fills in as events arrive.

**Verify in Terminal:**
- The broadcast and match lines for the intent you just composed.

---

## 🤖 3. Test AI Agent Layer (Ollama)

**Action:**
- Watch the Detail screen after broadcasting.

**Verify in Terminal:**
- Ollama-backed negotiation activity from the local agent (`agent.rs`). If no
  Ollama instance is reachable, the terminal logs a warning instead — see
  Troubleshooting below.

---

## 📡 4. Test Mesh Networking Layer

**Verify in Terminal:**
- `intent broadcast` on the sending node.
- On a peer: `peer discovered`, then `📬 Received Intent: ...`.
- Single-node, no peers: `⚠️ Note: No peers connected (Single-Node Mode).
  Intent processed locally.` — this is expected, not an error.

---

## ⚡ 5. Test NEAR Settlement Layer

A real trade needs two funded testnet accounts: follow
[docs/near-two-device-checklist.md](docs/near-two-device-checklist.md).

**Action:**
- Once the agents agree a price, press `SIGN YOUR SIDE` on both Detail screens.

**Verify in UI:**
- Both move to **Settled**, showing the **PROOF** panel with the same transaction
  hash. It opens at `https://testnet.nearblocks.io/txns/<hash>`.

---

## 🔌 6. Test Offline Mode (Optional)

1. Turn off your Wi-Fi / Internet.
2. Compose and broadcast a new intent.
3. **Result:** The app should still sign the intent and attempt to broadcast
   it to the local mesh, even without internet. A signed pair is held, and
   nothing reaches the chain.
4. Reconnect — the pair goes out once, within about 30 seconds.

---

## 🧪 7. Multi-Node Simulation (Single Folder!)

You can run two instances from the **same folder** without copying anything.

**Step 1: Start Node A (Default)**
```bash
npm run tauri dev
```

**Step 2: Start Node B (Port 1421)**
Open a **new terminal** in the same folder and run:
```bash
PORT=1421 npm run tauri dev -- --config src-tauri/tauri.node2.conf.json
```

**Verify:**
- Node A runs on port 1420
- Node B runs on port 1421
- They will automatically discover each other! 🟣🟣

---

## ❌ Troubleshooting

- **Ollama Error?**
  - Run `ollama serve` manually in a separate terminal to see detailed logs,
    or point the app at a remote instance with `CABALMESH_OLLAMA_URL`.
- **"Insufficient Peers"?**
  - This is normal for single-node testing. To test peer discovery, run
    `npm run tauri dev` on a second machine on the same Wi-Fi, or use the
    multi-node simulation above.
