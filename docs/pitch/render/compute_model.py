# -*- coding: utf-8 -*-
"""Generates docs/pitch/assets/compute-model.svg."""
import os, sys, pathlib
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
PITCH = pathlib.Path(__file__).resolve().parents[1]

import textwrap
from dimensions import (BG, PANEL, LINE, FAINT, TEXT, MUTED, MINT, MONO,
                     t, rect, line, arrow, ticks, header, chip)

W, H = 1280, 800
out = [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {H}" width="{W}" height="{H}" '
       f'role="img" aria-label="CabalMesh compute model — which device runs which workload">',
       rect(0, 0, W, H, BG, "none"),
       header(W, "COMPUTE MODEL · WHERE EACH WORKLOAD RUNS",
              "One deal, three machines. The phone you have, the node on your desk, the locker at your door.",
              "1 LIVE · 2 CONCEPT")]

cols = [
    dict(status="LIVE", tone=MINT, name="PHONE / DESKTOP", role="THE APP YOU ALREADY RUN",
         runs="App shell, identity keys, intent UI, transaction signing, guardian approvals.",
         silicon="Whatever the user already owns. Tauri 2 over one Rust core — iOS, Android, macOS.",
         budget="Battery-bound. Both boxes are optional to it: the app works with neither of them present.",
         keeps="the root Ed25519 key"),
    dict(status="CONCEPT", tone=MUTED, name="SHADOWBOX", role="MESH · MODEL · PROVER",
         runs="Three jobs in one enclosure: the mesh radio that carries other people's traffic, the local model that reads your sentence, and the prover.",
         silicon="8-core 2.4 GHz + ~20 TOPS NPU, 16 GB LPDDR5, 256 GB NVMe. BLE 5.3 + Wi-Fi 6, LoRa bay. Fanless.",
         budget="8B model at 4-bit, ~15 tok/s. 12 W idle, 25 W proving. 100-300 kbit/s shared across a room.",
         keeps="the prompt, the witness, the plaintext it never had"),
    dict(status="CONCEPT", tone=MUTED, name="THE NOBODY BOX", role="PHYSICAL ESCROW LOCKER",
         runs="Watches one deal. Verifies the on-chain release, drives the bolt, and weighs what is inside — so the app can say the item went in, not what it is.",
         silicon="Secure element + Cortex-M33 with TrustZone, BLE 5.3, motorised bolt, load cell in the floor.",
         budget="4 x D cells, about 12 months idle. The bolt draws for 1.5 s a cycle, the radio in bursts.",
         keeps="the latch key — it answers only to a settled deal"),
]

CW, CX0, GAP = 386, 40, 400
CY, CH = 110, 372

def wrap(s, n):
    return textwrap.wrap(s, n)

for i, c in enumerate(cols):
    x = CX0 + i * GAP
    out.append(rect(x, CY, CW, CH, PANEL, LINE))
    if i == 0:
        out.append(rect(x, CY, CW, CH, "none", MINT, 1, op=0.35))
    out.append(chip(x + 16, CY + 28, c["status"], c["tone"]))
    out.append(t(x + 16, CY + 62, c["name"], 15, TEXT, "start", 0.10, 500))
    out.append(t(x + 16, CY + 80, c["role"], 9, MINT, "start", 0.20))
    out.append(line(x + 16, CY + 94, x + CW - 16, CY + 94, FAINT))
    y = CY + 118
    for label, key in (("RUNS", "runs"), ("SILICON · TARGET", "silicon"), ("BUDGET", "budget")):
        out.append(t(x + 16, y, label, 8.5, MUTED, "start", 0.24))
        y += 17
        for lnn in wrap(c[key], 52):
            out.append(t(x + 16, y, lnn, 10.5, TEXT, "start", 0.01))
            y += 15
        y += 11
    out.append(line(x + 16, CY + CH - 54, x + CW - 16, CY + CH - 54, FAINT))
    out.append(t(x + 16, CY + CH - 36, "NEVER LEAVES", 8.5, MINT, "start", 0.24))
    ky = CY + CH - 20
    for lnn in wrap(c["keeps"], 52):
        out.append(t(x + 16, ky, lnn, 10.5, TEXT, "start", 0.01))
        ky += 14

# ── the path one deal takes ─────────────────────────────────────────────────
FY = 590
out.append(t(40, 528, "ONE DEAL'S PATH", 12, TEXT, "start", 0.22, 500))
out.append(t(215, 528, "— left to right, each step names the machine that runs it", 10, MUTED, "start", 0.02))
out.append(line(40, 540, W - 40, 540, LINE))

steps = [
    ("01", "PHONE", "TYPE", "\"Sell the lens for 2 AVAX, escrow it\" goes into the command bar."),
    ("02", "SHADOWBOX", "PARSE", "The local model turns the sentence into a structured intent and a price."),
    ("03", "SHADOWBOX", "PROVE", "A proof says the buyer's balance covers it — without naming the balance."),
    ("04", "SHADOWBOX", "FLOOD", "Intent and proof cross the BLE plane. TTL 7, dedup, jitter."),
    ("05", "ANY GATEWAY", "SETTLE", "The first node with Relay Mode on submits it; the payment locks in escrow."),
    ("06", "NOBODY BOX", "OPEN", "The bolt turns on that release. The load cell sees the weight leave, and the seller is paid."),
]
SW, SG, SH = 182, 22, 152
for i, (num, who, verb, body) in enumerate(steps):
    x = 40 + i * (SW + SG)
    out.append(rect(x, FY, SW, SH, PANEL, LINE))
    out.append(t(x + 14, FY + 28, num, 15, MINT, "start", 0.08, 500))
    out.append(t(x + 44, FY + 28, verb, 12, TEXT, "start", 0.18, 500))
    out.append(t(x + 14, FY + 46, who, 8.5, MINT, "start", 0.24))
    out.append(line(x + 14, FY + 58, x + SW - 14, FY + 58, FAINT))
    y = FY + 76
    for lnn in wrap(body, 26):
        out.append(t(x + 14, y, lnn, 10, TEXT, "start", 0.01))
        y += 14
    if i < len(steps) - 1:
        ax = x + SW
        out.append(line(ax + 3, FY + SH / 2, ax + SG - 7, FY + SH / 2, MINT, 1, op=0.75))
        out.append(arrow(ax + SG - 3, FY + SH / 2, 1, 0, 4.5, MINT))

# one bracket over steps 02-04: three of the six steps happen inside one box
bx1 = 40 + 1 * (SW + SG)
bx2 = 40 + 4 * (SW + SG) - SG
by = FY - 22
out.append(line(bx1, by, bx2, by, MINT, 1, op=0.55))
out.append(line(bx1, by, bx1, by + 10, MINT, 1, op=0.55))
out.append(line(bx2, by, bx2, by + 10, MINT, 1, op=0.55))
out.append(rect((bx1 + bx2) / 2 - 128, by - 12, 256, 17, BG, "none"))
out.append(t((bx1 + bx2) / 2, by + 1, "THREE OF THE SIX, INSIDE ONE BOX", 9, MINT, "middle", 0.22))

out.append(line(40, H - 66, W - 40, H - 66, LINE))
out.append(t(40, H - 44, "STATUS  The phone column is what ships today — see docs/product-status.md. Both boxes are Phase-2 concepts: no silicon selected, no board, no enclosure.", 9.5, MUTED, "start", 0.04))
out.append(t(40, H - 28, "GROUNDING  Mesh figures from docs/ble-mesh-design.md · local-model path from OLLAMA_INTEGRATION.md · escrow from contracts/contracts/Marketplace.sol · proving is Phase 4, no circuit in the repo", 9.5, MUTED, "start", 0.04))
out.append(ticks(40, 20, W - 80, H - 86, 8, MINT, 0.6))
out.append("</svg>")

open(PITCH / "assets" / "compute-model.svg", "w").write("\n".join(out))
print("wrote compute-model.svg")
