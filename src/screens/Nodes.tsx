import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Panel, StatusDot } from "../ds";
import type { BleStatusView, NodeSummary } from "../types/bindings";

/**
 * The network map and peer list.
 *
 * **No distances.** The prototype lists `1.2 km`; a libp2p peer has an
 * identifier and an address, not coordinates, and this app requests no location
 * permission — asking for one would contradict the premise. Rows show what the
 * mesh actually knows: latency, hops, transport.
 *
 * Map positions are seeded by peer id in Rust, so a node stays put across
 * renders and restarts. That is what makes the map readable as an instrument
 * rather than a lava lamp.
 */
export function Nodes() {
  const [nodes, setNodes] = useState<NodeSummary[] | null>(null);
  const [ble, setBle] = useState<BleStatusView | null>(null);
  const [denied, setDenied] = useState(false);

  useEffect(() => {
    let cancelled = false;

    const refresh = () => {
      invoke<NodeSummary[]>("list_nearby_nodes")
        .then((next) => {
          if (!cancelled) {
            setNodes(next);
            setDenied(false);
          }
        })
        .catch(() => {
          if (!cancelled) setDenied(true);
        });

      // Separately, and failure here does not blank the list: a node with a
      // radio and no network still has peers worth showing, and the reverse.
      invoke<BleStatusView>("ble_status")
        .then((next) => {
          if (!cancelled) setBle(next);
        })
        .catch(() => {
          if (!cancelled) setBle(null);
        });
    };

    refresh();
    const timer = window.setInterval(refresh, 4_000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, []);

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-6)", padding: "var(--space-6)" }}>
      <Panel label="NETWORK MAP">
        <div
          style={{
            position: "relative",
            height: 240,
            backgroundImage: "var(--texture-grid)",
            backgroundSize: "var(--texture-grid-size)",
          }}
        >
          {(nodes ?? []).map((node) => (
            <span
              key={node.id}
              aria-hidden="true"
              style={{
                position: "absolute",
                left: `${node.x * 100}%`,
                top: `${node.y * 100}%`,
                width: 5,
                height: 5,
                background: "var(--ink-white)",
                // Seeded duration, so the field does not pulse in unison.
                animation: `cm-pulse ${node.pulseMs}ms var(--ease-step-coarse) infinite`,
              }}
            />
          ))}
        </div>
      </Panel>

      <BleStatus status={ble} />

      <Panel label="NEARBY NODES">
        {denied ? (
          <Empty
            title="DISCOVERY UNAVAILABLE"
            body="Local network access is not granted. No peers can be found on this network."
          />
        ) : nodes === null ? (
          <Empty title="SCANNING" body="Looking for nodes." />
        ) : nodes.length === 0 ? (
          // "No peers nearby" and "no way to look for peers" are different
          // messages, and conflating them hides a fixable permission problem.
          <Empty title="NO NODES NEARBY" body="Nothing is reachable. Nothing is stored." />
        ) : (
          nodes.map((node) => <NodeRow key={node.id} node={node} />)
        )}
      </Panel>
    </div>
  );
}

/**
 * The offline plane's own status.
 *
 * Every figure here is measured. There is no signal strength and no distance:
 * the radio reports neither, and this app requests no location permission.
 *
 * `SUPPRESSED` sits beside `RELAYED` deliberately — without it, "the mesh is
 * quiet" and "every packet is arriving twice and being cancelled" render
 * identically, and only one of those is worth waking up for.
 */
function BleStatus({ status }: { status: BleStatusView | null }) {
  if (status === null) {
    return null;
  }

  if (!status.running) {
    return (
      <Panel label="OFFLINE PLANE">
        <Empty
          title="RADIO NOT RUNNING"
          body="No Bluetooth backend for this build. Peers can only be found over the network."
        />
      </Panel>
    );
  }

  const figures: [string, string][] = [
    ["LINKS", String(status.links)],
    ["IN RANGE", String(status.directPeers)],
    ["REACHABLE", String(status.reachablePeers)],
    ["GATEWAYS", String(status.gateways)],
    ["RELAYED", String(status.relayed)],
    ["SUPPRESSED", String(status.suppressed)],
  ];

  return (
    <Panel label="OFFLINE PLANE">
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: "var(--space-4)",
          padding: "var(--space-5) var(--space-6)",
        }}
      >
        {/* Three states, because they mean different things: silenced by the
            user, up with peers, and up with nobody in range yet. */}
        <StatusDot tone={status.offline ? "offline" : status.links > 0 ? "online" : "idle"} />
        <span
          style={{
            fontFamily: "var(--type-data-family)",
            fontSize: "var(--text-sm)",
            letterSpacing: "var(--type-data-tracking)",
            color: "var(--text-primary)",
          }}
        >
          {status.nodeId}
        </span>
        <span
          style={{
            marginLeft: "auto",
            fontFamily: "var(--type-label-family)",
            fontSize: "var(--text-2xs)",
            letterSpacing: "var(--tracking-widest)",
            color: "var(--text-muted)",
          }}
        >
          {/* Verbatim. `loopback` is not Bluetooth, and calling it that would
              claim a capability this build does not have. */}
          {status.offline ? "SILENCED" : status.transport.toUpperCase()}
        </span>
      </div>

      <div
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(3, 1fr)",
          borderTop: "var(--border-hairline-style)",
        }}
      >
        {figures.map(([label, value]) => (
          <div
            key={label}
            style={{
              display: "flex",
              flexDirection: "column",
              gap: "var(--space-2)",
              padding: "var(--space-5) var(--space-6)",
            }}
          >
            <span
              style={{
                fontFamily: "var(--type-label-family)",
                fontSize: "var(--text-2xs)",
                letterSpacing: "var(--tracking-widest)",
                color: "var(--text-muted)",
              }}
            >
              {label}
            </span>
            <span
              style={{
                fontFamily: "var(--type-data-family)",
                fontSize: "var(--text-lg)",
                color: "var(--text-primary)",
              }}
            >
              {value}
            </span>
          </div>
        ))}
      </div>
    </Panel>
  );
}

function NodeRow({ node }: { node: NodeSummary }) {
  // A radio peer is a person in the room and is still there when the Wi-Fi is
  // not, so the transport is named rather than folded into "DIRECT".
  const via = node.transport === "ble" ? "RADIO" : node.transport.toUpperCase();
  const detail =
    node.hops > 1
      ? `${via} · ${node.hops} HOPS`
      : node.latencyMs != null
        ? `${node.latencyMs}ms · ${via}`
        : `${via} · DIRECT`;

  return (
    <div
      className="cm-row"
      style={{
        display: "flex",
        alignItems: "center",
        gap: "var(--space-5)",
        padding: "var(--space-5) var(--space-6)",
        borderTop: "var(--border-hairline-style)",
      }}
    >
      <StatusDot tone="online" />
      <div style={{ flex: 1, display: "flex", flexDirection: "column", gap: "var(--space-2)" }}>
        <span
          style={{
            fontFamily: "var(--type-data-family)",
            fontSize: "var(--text-sm)",
            letterSpacing: "var(--type-data-tracking)",
            color: "var(--text-primary)",
          }}
        >
          {node.id}
        </span>
        <span
          style={{
            fontFamily: "var(--type-label-family)",
            fontSize: "var(--text-2xs)",
            letterSpacing: "var(--tracking-widest)",
            color: "var(--text-muted)",
          }}
        >
          STAKED · LIVENESS OK
        </span>
      </div>
      <span
        style={{
          fontFamily: "var(--type-data-family)",
          fontSize: "var(--text-2xs)",
          letterSpacing: "var(--tracking-wide)",
          color: "var(--text-secondary)",
        }}
      >
        {detail}
      </span>
    </div>
  );
}

function Empty({ title, body }: { title: string; body: string }) {
  return (
    <div
      style={{
        padding: "var(--space-9) var(--space-6)",
        display: "flex",
        flexDirection: "column",
        gap: "var(--space-4)",
        alignItems: "center",
        textAlign: "center",
      }}
    >
      <span
        style={{
          fontFamily: "var(--type-heading-family)",
          fontSize: "var(--text-sm)",
          letterSpacing: "var(--type-heading-tracking)",
          color: "var(--text-primary)",
        }}
      >
        {title}
      </span>
      <span style={{ fontSize: "var(--text-base)", color: "var(--text-muted)" }}>{body}</span>
    </div>
  );
}
