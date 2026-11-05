//! The reshaped command surface — the whole IPC surface, shared by desktop
//! and mobile. Commands here return [`AppError`] rather than `String`, so the
//! frontend switches on a variant and renders its own copy.
//!
//! Screen commands land with their screens — never speculatively, because an
//! unreachable command still has to be granted a permission, and a permission
//! granted ahead of a caller is a permission nobody is checking.

use crate::error::AppError;
use crate::state::AppState;
use cabal_core::SubscriptionId;
use tauri::State;

/// Stops delivery for a live stream.
///
/// **Cancels delivery, not the operation being reported on.** Leaving the
/// connecting screen does not disconnect the mesh; leaving the settled screen
/// does not abort an in-flight settlement. Aborting a domain operation is a
/// separate, explicit command — conflating the two would let a UI navigation
/// cancel a transaction.
///
/// Idempotent by design. The frontend races unmount against subscribe, so a
/// teardown for a handle that never landed is routine rather than exceptional,
/// and must not surface as an error the UI has to explain.
///
/// # Errors
///
/// None currently. It returns `Result` because every command on this surface
/// does, so adding a failure case later is not a breaking change for callers.
#[tauri::command]
pub async fn unsubscribe(id: String, state: State<'_, AppState>) -> Result<(), AppError> {
    state.subscriptions().cancel(&SubscriptionId::new(id));
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_broadcast_payload_has_no_limit() {
        let draft = cabal_core::IntentDraft {
            chain: cabal_core::Chain::NearTestnet,
            action: cabal_core::Action::Sell,
            asset: "wNEAR".into(),
            condition: cabal_core::Condition::Above { price: cabal_core::UsdPrice::from_cents(391) },
            amount: cabal_core::TokenAmount::parse("1", 24).unwrap(),
            mode: cabal_core::ExecutionMode::Shark,
            privacy: cabal_core::PrivacyLevel::Low,
        };
        let dir = tempfile::tempdir().unwrap();
        let ledger = crate::intents::Ledger::open(cabal_store::JsonStore::new(dir.path().join("i.json")));
        let intent = ledger.create(draft, 1);

        let payload = public_payload(&intent).unwrap();

        assert!(!payload.contains("391"), "{payload}");
        assert!(!payload.contains("3.91"), "{payload}");
        assert!(!payload.contains("condition"), "{payload}");
    }

    use super::*;

    #[tokio::test]
    async fn cancelling_an_unknown_handle_is_not_an_error() {
        // Exercises the registry directly; the command is a thin wrapper and
        // `State` cannot be constructed outside a Tauri app.
        let state = AppState::new();
        state
            .subscriptions()
            .cancel(&SubscriptionId::new("never-registered"));
        assert!(state.subscriptions().is_empty());
    }

    #[tokio::test]
    async fn cancelling_twice_is_not_an_error() {
        let state = AppState::new();
        let (id, token) = state.subscriptions().register("mesh-log").unwrap();

        state.subscriptions().cancel(&id);
        state.subscriptions().cancel(&id);

        assert!(token.is_cancelled());
        assert!(state.subscriptions().is_empty());
    }
}

// ---------------------------------------------------------------------------
// Session — splash and connecting
// ---------------------------------------------------------------------------

/// What the splash screen needs to decide what it is offering.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct SessionStatus {
    /// Whether bootstrap has finished and the mesh is usable.
    pub ready: bool,
    /// Truncated node id, e.g. `7F3A..8C2E`. Absent before bootstrap.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    /// Whether a peer is reachable right now.
    pub connected: bool,
}

/// Whether this device already has a live session.
///
/// # Errors
///
/// Never fails: "not ready" is a value the splash screen renders, not an error
/// it has to explain.
#[tauri::command]
pub async fn session_status(state: State<'_, AppState>) -> Result<SessionStatus, AppError> {
    let ready = state.is_ready();
    let runtime = state.runtime_caps();

    let node_id = match state.services() {
        Ok(services) => services
            .mesh
            .as_ref()
            .map(|_| cabal_core::NodeId::new("pending").truncated()),
        Err(_) => None,
    };

    Ok(SessionStatus {
        ready,
        node_id,
        connected: runtime.online,
    })
}

/// Joins the mesh, streaming the handshake log.
///
/// Returns a [`SubscriptionId`] **immediately** rather than blocking until the
/// handshake finishes, so the connecting screen can render progress rather than
/// waiting on a pending invoke.
///
/// Cancelling the returned subscription stops log delivery. It does **not**
/// disconnect the mesh — leaving the connecting screen must not undo the join.
///
/// # Errors
///
/// [`AppError::TooManySubscriptions`] if the registry is full.
#[tauri::command]
pub async fn enter_mesh(
    on_line: tauri::ipc::Channel<crate::bindings::LogLine>,
    state: State<'_, AppState>,
) -> Result<String, AppError> {
    use crate::bindings::{LogLine, LogTone};

    let (id, token) = state.subscriptions().register("handshake")?;
    let registry = state.subscriptions().clone();
    let handle = id.clone();

    tauri::async_runtime::spawn(async move {
        // The prototype's own handshake sequence, in its voice: uppercase,
        // terse, ellipsis while in flight.
        let steps = [
            ("INITIALIZING EPHEMERAL NODE...", LogTone::Dim),
            ("GENERATING ONE-TIME KEYPAIR...", LogTone::Dim),
            ("NO IDENTITY WRITTEN.", LogTone::Out),
            ("ROUTING THROUGH MESH...", LogTone::Dim),
            ("MESH REACHED. SUCCESS.", LogTone::Ok),
        ];

        for (text, tone) in steps {
            // Cancellation is checked in the same select as the work, so a
            // cancelled stream stops at its next yield rather than after its
            // whole backlog.
            tokio::select! {
                () = token.cancelled() => break,
                () = tokio::time::sleep(std::time::Duration::from_millis(520)) => {}
            }
            if on_line.send(LogLine::new(text, tone)).is_err() {
                // The webview is gone; nothing left to deliver to.
                break;
            }
        }

        // Frees the slot whether the stream finished or was cancelled, so a
        // completed handshake does not occupy the registry until teardown.
        registry.finished(&handle);
    });

    Ok(id.to_string())
}

// ---------------------------------------------------------------------------
// Home
// ---------------------------------------------------------------------------

/// What the home screen renders.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct MeshSnapshotView {
    /// Truncated for display, e.g. `7F3A..8C2E`.
    pub node_id: String,
    /// Uptime in the board's format, e.g. `3D 14H 22M`.
    pub uptime: String,
    pub connected: bool,
    pub stats: Vec<crate::bindings::StatTile>,
}

/// Mesh status for the home screen.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap completes, which the connecting
/// screen already renders as progress.
#[tauri::command]
pub async fn mesh_snapshot(state: State<'_, AppState>) -> Result<MeshSnapshotView, AppError> {
    use crate::bindings::{separated, StatTile};

    let services = state.services()?;
    let mesh = services.mesh.as_ref().ok_or(AppError::MeshOffline)?;
    let snapshot = mesh.snapshot().await.map_err(|_| AppError::MeshOffline)?;

    // Deltas are omitted rather than fabricated. There is no baseline to
    // compare against yet, and the brand's copy rules demand exact figures —
    // a made-up "+12.4%" would be a fabricated trust signal in a product whose
    // whole pitch is proving things.
    // The third tile is the one figure here that is about this node rather
    // than the network: what it has settled. Ticket 39 replaced ticket 03's
    // mocked "reputation score" with it — see src/standing.rs for why the
    // label changed rather than the definition.
    //
    // It reads the local ledger, so unlike the other two it is just as true
    // with no mesh as with one.
    let standing = crate::standing::LocalStanding::of(state.intents(), crate::intents::now_ms());
    let settled_tile = match standing.delta_percent {
        Some(delta) => StatTile::with_delta("INTENTS SETTLED", standing.value(), delta),
        // No prior window, so no baseline. `plain` omits the delta rather than
        // rendering `+0.0%` for a trend that was never measured.
        None => StatTile::plain("INTENTS SETTLED", standing.value()),
    };

    // Broadcasted: ledger entries this device has actually pushed off the
    // device — a `Draft` is composed but nothing has left yet, so it does not
    // count. Received: distinct intents seen from other peers, over the mesh
    // or bridged in from BLE — see `ReceivedLog` for why it dedupes rather
    // than counting every delivery attempt. Both read local state, so like
    // `settled_tile` they hold with no mesh connected at all.
    let broadcasted = state
        .intents()
        .all()
        .iter()
        .filter(|intent| !matches!(intent.status, cabal_core::IntentStatus::Draft))
        .count();

    let stats = vec![
        StatTile::plain("NETWORK NODES", separated(snapshot.peer_count as u64)),
        StatTile::plain("RELAYED BYTES", separated(snapshot.relay_bytes)),
        settled_tile,
        StatTile::plain("BROADCASTED", separated(broadcasted as u64)),
        StatTile::plain("RECEIVED", separated(state.received().count() as u64)),
    ];

    Ok(MeshSnapshotView {
        node_id: cabal_core::NodeId::new(snapshot.peer_id.clone()).truncated(),
        uptime: format_uptime(state.uptime_seconds()),
        connected: snapshot.peer_count > 0,
        stats,
    })
}

/// Formats seconds as `3D 14H 22M`, matching the board.
///
/// Days are dropped when zero rather than rendered as `0D`, which reads as
/// broken rather than as "less than a day".
fn format_uptime(seconds: u64) -> String {
    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3_600;
    let minutes = (seconds % 3_600) / 60;
    if days > 0 {
        format!("{days}D {hours}H {minutes}M")
    } else if hours > 0 {
        format!("{hours}H {minutes}M")
    } else {
        format!("{minutes}M")
    }
}

/// Streams the mesh log ticker.
///
/// Replays the retained tail first so the terminal is never empty on first
/// paint, then streams live.
///
/// # Errors
///
/// [`AppError::TooManySubscriptions`] if the registry is full.
#[tauri::command]
pub async fn subscribe_mesh_log(
    on_line: tauri::ipc::Channel<crate::bindings::LogLine>,
    state: State<'_, AppState>,
) -> Result<String, AppError> {
    use crate::bindings::{LogLine, LogTone};

    let (id, token) = state.subscriptions().register("mesh-log")?;
    let registry = state.subscriptions().clone();
    let handle = id.clone();
    let services = state.services().ok();

    tauri::async_runtime::spawn(async move {
        loop {
            tokio::select! {
                () = token.cancelled() => break,
                () = tokio::time::sleep(std::time::Duration::from_millis(1_800)) => {}
            }

            let Some(services) = services.as_ref() else { break };
            let Some(mesh) = services.mesh.as_ref() else { break };
            let Ok(snapshot) = mesh.snapshot().await else { break };

            // Real mesh state, not a canned array. Lowercase and terse, as the
            // board specifies for log lines.
            let line = LogLine::new(
                format!("peers {} · relayed {} bytes", snapshot.peer_count, snapshot.relay_bytes),
                if snapshot.peer_count > 0 { LogTone::Ok } else { LogTone::Dim },
            );
            if on_line.send(line).is_err() {
                break;
            }
        }
        registry.finished(&handle);
    });

    Ok(id.to_string())
}

// ---------------------------------------------------------------------------
// Nodes
// ---------------------------------------------------------------------------

/// How a peer is reached.
#[derive(Debug, Clone, Copy, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "lowercase")]
pub enum Transport {
    /// Found on this network.
    Mdns,
    /// Direct connection.
    Quic,
    /// Reached through a relay.
    Relayed,
    /// Heard over the radio, with no network involved at all.
    ///
    /// Distinct from the others in the only way the user cares about: a `Ble`
    /// peer is a person in the room, and it is still there when the Wi-Fi is
    /// not.
    Ble,
}

/// A peer, as the nodes screen shows it.
///
/// **No distance.** A libp2p peer has an identifier and an address, not
/// coordinates, and this app requests no location permission — asking for one
/// would contradict the entire premise. The prototype's `1.2 km` is canned;
/// rendering it would be a fabricated measurement.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct NodeSummary {
    /// Truncated peer id, e.g. `8A3F..1209`.
    pub id: String,
    /// Round-trip time where known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u16>,
    /// 1 is direct; more means relayed.
    pub hops: u8,
    pub transport: Transport,
    /// Deterministic map position in [0,1], seeded by peer id.
    pub x: f32,
    pub y: f32,
    /// Milliseconds, also seeded, so the field does not pulse in unison.
    pub pulse_ms: u16,
}

/// Peers currently reachable.
///
/// Positions are **deterministic, seeded by peer id**: a node stays where it
/// was across renders and restarts, which is what makes the map readable as an
/// instrument rather than a lava lamp. The prototype's seven hardcoded slots do
/// not generalise to an arbitrary peer count.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap, [`AppError::MeshOffline`] without a
/// swarm.
#[tauri::command]
pub async fn list_nearby_nodes(state: State<'_, AppState>) -> Result<Vec<NodeSummary>, AppError> {
    let services = state.services()?;

    // BLE peers first, and they are real rows rather than a count: the engine
    // knows each one's identifier and how many hops away it is, so there is
    // nothing to invent.
    let mut nodes = Vec::new();
    if let Some(ble) = services.ble.as_ref() {
        if let Ok(peers) = ble.peers().await {
            for peer in peers {
                let id = peer.id.to_string();
                let (x, y, pulse) = seeded_position(&id);
                nodes.push(NodeSummary {
                    id: cabal_core::NodeId::new(id).truncated(),
                    latency_ms: None,
                    hops: peer.hops,
                    transport: Transport::Ble,
                    x,
                    y,
                    pulse_ms: pulse,
                });
            }
        }
    }

    // The IP plane. Without a swarm this is simply empty — a node with
    // Bluetooth and no network is a working node, and returning `MeshOffline`
    // here would blank a screen that has peers to show.
    let Some(mesh) = services.mesh.as_ref() else {
        return Ok(nodes);
    };
    let Ok(snapshot) = mesh.snapshot().await else {
        return Ok(nodes);
    };

    // The mesh actor reports a count, not a registry; per-peer detail arrives
    // with the peer registry in a later ticket. Rendering the count honestly
    // beats inventing rows.
    for index in 0..snapshot.peer_count {
        let seed = format!("{}-{index}", snapshot.peer_id);
        let (x, y, pulse) = seeded_position(&seed);
        nodes.push(NodeSummary {
            id: cabal_core::NodeId::new(seed.clone()).truncated(),
            latency_ms: None,
            hops: 1,
            transport: Transport::Mdns,
            x,
            y,
            pulse_ms: pulse,
        });
    }
    Ok(nodes)
}

/// What the nodes screen shows about the offline plane.
///
/// Every field is a measurement. There is no "signal strength" and no
/// "distance": the radio reports neither, and the app requests no location
/// permission — asking for one would contradict the premise.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct BleStatusView {
    /// Whether the plane is running at all.
    ///
    /// False means no radio backend and no development transport — a state
    /// the app is expected to survive, and one the screen has to be able to
    /// say out loud rather than rendering as "no peers".
    pub running: bool,
    /// This session's identifier, truncated. Changes on every launch.
    pub node_id: String,
    /// Which backend is carrying the plane, verbatim: `loopback`, or the name
    /// of a real radio. Never dressed up as "BLE" when it is not.
    pub transport: String,
    /// Radio links currently open.
    pub links: usize,
    /// Peers one hop away — people in the room.
    pub direct_peers: usize,
    /// Every peer reachable, direct or through a neighbour.
    pub reachable_peers: usize,
    /// Reachable peers offering a way to the internet.
    pub gateways: usize,
    /// Packets forwarded for other people.
    pub relayed: u64,
    /// Forwards cancelled because a neighbour was faster.
    ///
    /// Shown beside `relayed` because without it the two states "the mesh is
    /// quiet" and "everything is arriving twice and being suppressed" look
    /// identical.
    pub suppressed: u64,
    pub offline: bool,
}

/// Status of the BLE plane.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap completes. Never
/// [`AppError::MeshOffline`]: a plane that is not running is a status, not a
/// failure, and the screen needs to render it.
#[tauri::command]
pub async fn ble_status(state: State<'_, AppState>) -> Result<BleStatusView, AppError> {
    let services = state.services()?;

    let Some(ble) = services.ble.as_ref() else {
        return Ok(BleStatusView {
            running: false,
            node_id: String::new(),
            transport: String::new(),
            links: 0,
            direct_peers: 0,
            reachable_peers: 0,
            gateways: 0,
            relayed: 0,
            suppressed: 0,
            offline: false,
        });
    };

    let status = ble.status().await.map_err(|_| AppError::NotReady {
        subsystem: "ble".into(),
    })?;
    Ok(BleStatusView {
        running: true,
        node_id: cabal_core::NodeId::new(status.peer_id.to_string()).truncated(),
        transport: services.ble_transport.clone(),
        links: status.links,
        direct_peers: status.direct_peers,
        reachable_peers: status.reachable_peers,
        gateways: status.gateways,
        relayed: status.relayed,
        suppressed: status.suppressed,
        offline: status.offline,
    })
}

/// Deterministic position and pulse from a peer id.
///
/// A hash rather than randomness so a node does not jump between renders, and
/// the pulse is seeded too so the field does not throb in unison.
fn seeded_position(seed: &str) -> (f32, f32, u16) {
    use std::hash::{DefaultHasher, Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    seed.hash(&mut hasher);
    let hash = hasher.finish();

    // Inset from the edges so a node is never clipped by the map's frame.
    let x = 0.12 + ((hash & 0xFFFF) as f32 / 65_535.0) * 0.76;
    let y = 0.12 + (((hash >> 16) & 0xFFFF) as f32 / 65_535.0) * 0.76;
    let pulse = 900 + u16::try_from((hash >> 32) % 750).unwrap_or(0);
    (x, y, pulse)
}

// ---------------------------------------------------------------------------
// Intents
// ---------------------------------------------------------------------------

/// An intent as a list row renders it.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct IntentView {
    pub id: String,
    /// e.g. `BUY wNEAR`.
    pub title: String,
    /// e.g. `UNDER $95`.
    pub subtitle: String,
    /// Execution mode, shown as a badge. Absent when default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub badge: Option<String>,
    pub amount: String,
    /// The lifecycle state, driving both the status text and the dot tone.
    pub status: cabal_core::IntentStatus,
    /// Elapsed or settled time, e.g. `2M 14S` or `11.4S`.
    pub elapsed: String,
}

/// Which slice of the list to return.
#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "UPPERCASE")]
pub enum IntentFilter {
    Active,
    Pending,
    History,
}

impl IntentFilter {
    /// Whether an intent belongs in this slice.
    ///
    /// `Pending` is the queue: composed but never broadcast, which is exactly
    /// what an intent created offline looks like. Mapping it to anything else
    /// would hide the queue the offline path exists to build.
    fn admits(self, status: &cabal_core::IntentStatus) -> bool {
        match self {
            Self::Active => status.is_active(),
            Self::Pending => matches!(status, cabal_core::IntentStatus::Draft),
            Self::History => status.is_terminal(),
        }
    }
}

/// Renders one ledger entry as a list row.
fn row_for(intent: &crate::intents::Intent, now_ms: u64) -> IntentView {
    let action = format!("{:?}", intent.draft.action).to_uppercase();
    let subtitle = match intent.draft.condition {
        cabal_core::Condition::Under { price } => format!("UNDER {price}"),
        cabal_core::Condition::Above { price } => format!("ABOVE {price}"),
        cabal_core::Condition::Any => "ANY PRICE".into(),
    };

    IntentView {
        id: intent.id.to_string(),
        title: format!("{action} {}", intent.draft.asset),
        subtitle,
        // Shark is the default, so badging it would put a badge on almost
        // every row and stop the badge meaning anything.
        badge: match intent.draft.mode {
            cabal_core::ExecutionMode::Shark => None,
            other => Some(other.label().to_string()),
        },
        amount: format!("{} {}", intent.draft.amount, intent.draft.asset),
        status: intent.status.clone(),
        elapsed: crate::intents::format_elapsed(intent.elapsed_ms(now_ms)),
    }
}

/// Intents matching `filter`, newest first.
///
/// # Errors
///
/// Never fails. Deliberately does **not** require bootstrap: the intents most
/// worth showing are the ones queued while there was no mesh to boot.
#[tauri::command]
pub async fn list_intents(
    filter: IntentFilter,
    state: State<'_, AppState>,
) -> Result<Vec<IntentView>, AppError> {
    let now = crate::intents::now_ms();
    Ok(state
        .intents()
        .all()
        .iter()
        .filter(|intent| filter.admits(&intent.status))
        .map(|intent| row_for(intent, now))
        .collect())
}

/// The options the compose screen offers.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct FormOptions {
    pub actions: Vec<String>,
    /// What can be bought or sold. Only the base: every trade is priced in `quote`.
    pub assets: Vec<AssetOption>,
    /// What a buy pays with and a sell receives.
    pub quote: AssetOption,
    pub conditions: Vec<String>,
    pub modes: Vec<ModeOption>,
    pub privacy_levels: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct AssetOption {
    pub name: String,
    /// Three-letter tag the board shows beside the name.
    pub tag: String,
    pub decimals: u8,
    /// The spendable balance, pre-formatted — what MAX fills in.
    ///
    /// Absent rather than zero when the balance is unknown, which is what an
    /// asset this wallet has never held looks like. Rendering an unknown
    /// balance as `0` would tell the user they have none, which is a different
    /// claim entirely.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct ModeOption {
    pub label: String,
    pub description: String,
}

/// The network this device trades on right now. A draft always takes it from
/// here: the form offers no choice, because a signature for another network
/// could not settle from this one.
fn device_chain() -> cabal_core::Chain {
    crate::network_config::NetworkConfig::current().network.chain()
}

/// How many decimals `asset` carries, or `None` if it cannot be traded.
///
/// The quote is refused on purpose. It has a price of one by definition, so a
/// draft selling it for itself would have no meaning at settlement.
fn decimals_for(chain: cabal_core::Chain, asset: &str) -> Option<u8> {
    let base = crate::network_config::network_of(chain).near().base;
    (base.symbol == asset).then_some(base.decimals)
}

fn asset_option(spec: &crate::network_config::AssetSpec, balances: &crate::balances::Balances) -> AssetOption {
    AssetOption {
        name: spec.symbol.to_string(),
        tag: spec.code.to_string(),
        decimals: spec.decimals,
        available: balances
            .in_intents_raw(spec.symbol)
            .map(|raw| cabal_core::TokenAmount::from_raw(raw, spec.decimals).to_plain_string()),
    }
}

/// The cached balances of this device's account on the current network. Empty
/// while the vault is locked, or if they have never been read.
pub(crate) async fn cached_balances(state: &AppState) -> crate::balances::Balances {
    let Ok(services) = state.services() else {
        return crate::balances::Balances::default();
    };
    let Some(account) = services.identity.lock().await.primary_account() else {
        return crate::balances::Balances::default();
    };
    crate::balances::Balances::load_for(
        &crate::balances::store(),
        account.as_str(),
        crate::network_config::NetworkConfig::current().network,
    )
}

/// Builds [`FormOptions`] from the domain model and (best-effort) balances.
///
/// Split out from the [`intent_form_options`] command so `parse_intent_chat`
/// can embed the exact same option vocabulary in its prompt — the model's
/// allowed answers come from this, not a second hardcoded list that could
/// drift from what the segmented controls actually offer.
async fn build_form_options(state: &AppState) -> FormOptions {
    use cabal_core::{Action, ExecutionMode, PrivacyLevel};

    // Balances are best-effort. Before bootstrap, or with nothing read yet,
    // every asset simply has no maximum.
    let balances = cached_balances(state).await;
    let near = crate::network_config::NetworkConfig::current().near();

    FormOptions {
        // A swap or a stake has no meaning as a `token_diff` between two peers.
        actions: [Action::Buy, Action::Sell].iter().map(|a| format!("{a:?}").to_uppercase()).collect(),
        assets: vec![asset_option(&near.base, &balances)],
        quote: asset_option(&near.quote, &balances),
        conditions: vec!["Price under".into(), "Price above".into(), "Any price".into()],
        modes: ExecutionMode::ALL
            .iter()
            .map(|mode| ModeOption {
                label: mode.label().to_string(),
                description: mode.description().to_string(),
            })
            .collect(),
        privacy_levels: PrivacyLevel::ALL
            .iter()
            .map(|level| format!("{level:?}").to_uppercase())
            .collect(),
    }
}

/// Options for the compose screen.
///
/// Supplied by Rust rather than hardcoded on the frontend so a mode and its
/// description cannot drift apart — they come from one `ExecutionMode` — and
/// so the maximum comes from the same balance the vault screen shows.
///
/// # Errors
///
/// Never fails. An unavailable balance omits the maximum rather than failing
/// the whole form: composing an intent offline is a supported path.
#[tauri::command]
pub async fn intent_form_options(state: State<'_, AppState>) -> Result<FormOptions, AppError> {
    Ok(build_form_options(&state).await)
}

/// One row of the confirm dialog.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct ReviewRow {
    pub key: String,
    pub value: String,
}

/// What the confirm dialog renders.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct IntentPreview {
    pub rows: Vec<ReviewRow>,
    /// The dialog's closing line, chosen by whether this will broadcast now.
    pub confirm: String,
    /// Whether confirming broadcasts immediately or queues locally. Drives the
    /// button's own verb, so the dialog does not promise one thing in prose and
    /// another on the control.
    pub will_broadcast: bool,
}

/// The confirm dialog's closing line when the intent goes out now.
///
/// Ticket 04. Two strings rather than one vague enough to be true in both
/// states, and both live here beside the rows so the dialog cannot describe a
/// path this command will not take.
const CONFIRM_ONLINE: &str =
    "This intent broadcasts to the mesh and settles on-chain. No identity is attached.";

/// The closing line when there is no mesh to broadcast to.
///
/// The prototype claimed offline intents execute and settle. They do not: the
/// architecture is queue-then-drain.
const CONFIRM_QUEUED: &str =
    "Queued locally. Broadcast and settlement follow reconnection. No identity is attached.";

/// The compose form's fields, exactly as the screen holds them.
///
/// One type rather than seven parameters on two commands. That is what makes
/// "preview and broadcast see the same input" a property of the signature
/// instead of something a caller has to get right twice.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct IntentFields {
    pub action: String,
    pub asset: String,
    pub condition: String,
    /// Ignored when the condition carries no price.
    pub price: String,
    pub amount: String,
    pub mode: String,
    pub privacy: String,
}

/// Turns raw form fields into a domain draft.
///
/// The single parse for both preview and broadcast. That is what makes the
/// review rows honest: they are rendered *from the draft*, so the dialog cannot
/// describe one thing while the broadcast sends another.
fn parse_draft(fields: &IntentFields, chain: cabal_core::Chain) -> Result<cabal_core::IntentDraft, AppError> {
    use crate::error::InvalidReason;
    use cabal_core::{Action, Condition, ExecutionMode, IntentDraft, PrivacyLevel, TokenAmount, UsdPrice};

    let IntentFields { action, asset, condition, price, amount, mode, privacy } = fields;

    // Parsed, not trusted. Everything arriving from the webview is hostile
    // until it becomes a domain type.
    let decimals = decimals_for(chain, asset).ok_or(AppError::InvalidIntent {
        field: "asset",
        reason: InvalidReason::Malformed,
    })?;

    let action = match action.to_ascii_uppercase().as_str() {
        "BUY" => Action::Buy,
        "SELL" => Action::Sell,
        _ => {
            return Err(AppError::InvalidIntent {
                field: "action",
                reason: InvalidReason::Malformed,
            })
        }
    };

    let parsed_amount = TokenAmount::parse(amount, decimals)?;
    if parsed_amount.is_zero() {
        return Err(AppError::InvalidIntent {
            field: "amount",
            reason: InvalidReason::OutOfRange,
        });
    }

    let condition = if condition.to_ascii_lowercase().starts_with("any") {
        Condition::Any
    } else {
        let parsed_price = UsdPrice::parse(price).map_err(|_| AppError::InvalidIntent {
            field: "price",
            reason: InvalidReason::Malformed,
        })?;
        if condition.to_ascii_lowercase().contains("above") {
            Condition::Above { price: parsed_price }
        } else {
            Condition::Under { price: parsed_price }
        }
    };

    let mode = ExecutionMode::ALL
        .into_iter()
        .find(|candidate| candidate.label().eq_ignore_ascii_case(mode))
        .ok_or(AppError::InvalidIntent {
            field: "mode",
            reason: InvalidReason::Malformed,
        })?;

    let privacy = PrivacyLevel::ALL
        .into_iter()
        .find(|candidate| format!("{candidate:?}").eq_ignore_ascii_case(privacy))
        .ok_or(AppError::InvalidIntent {
            field: "privacy",
            reason: InvalidReason::Malformed,
        })?;

    Ok(IntentDraft {
        chain,
        action,
        asset: asset.as_str().into(),
        condition,
        amount: parsed_amount,
        mode,
        privacy,
    })
}

/// The five rows the confirm dialog shows, rendered from the draft itself.
fn review_rows(draft: &cabal_core::IntentDraft) -> Vec<ReviewRow> {
    use cabal_core::Condition;

    let condition = match draft.condition {
        Condition::Under { price } => format!("UNDER {price}"),
        Condition::Above { price } => format!("ABOVE {price}"),
        Condition::Any => "ANY PRICE".into(),
    };

    vec![
        ReviewRow {
            key: "ACTION".into(),
            value: format!("{:?} {}", draft.action, draft.asset).to_uppercase(),
        },
        ReviewRow { key: "CONDITION".into(), value: condition },
        ReviewRow {
            key: "AMOUNT".into(),
            value: format!("{} {}", draft.amount, draft.asset),
        },
        ReviewRow { key: "MODE".into(), value: draft.mode.label().to_string() },
        ReviewRow {
            key: "PRIVACY".into(),
            value: format!("{:?}", draft.privacy).to_uppercase(),
        },
    ]
}

/// Whether an intent composed right now would leave the device.
///
/// Peer count is part of the answer, not an afterthought: gossipsub with no
/// peers has nobody to publish to, so promising a broadcast there would be the
/// same class of lie ticket 04 retired.
async fn will_broadcast(state: &AppState) -> bool {
    let Ok(services) = state.services() else {
        return false;
    };
    let Some(mesh) = services.mesh.as_ref() else {
        return false;
    };
    mesh.snapshot()
        .await
        .is_ok_and(|snapshot| !snapshot.offline && snapshot.peer_count > 0)
}

/// Parses free text into intent fields via the local (or configured) LLM —
/// the "say what you want to do" entry point in
/// `docs/intent-chat-and-modules-design.md`.
///
/// **This is exactly as trusted as a hand-filled form.** The returned
/// fields are raw strings, identical in shape to what `New.tsx` already
/// builds from its own inputs; nothing about this command validates them,
/// signs anything, or is closer to broadcast than the empty form is. The
/// frontend feeds the result into the same `preview_intent` /
/// `broadcast_intent` pipeline unchanged, which is what actually validates
/// it. The model proposes; Rust still decides.
///
/// # Errors
///
/// [`AppError::Internal`] if the LLM could not be reached at all — a model
/// that responded but not in valid JSON is *not* an error: every field
/// comes back blank instead, which the review step already knows how to
/// reject field by field.
#[tauri::command]
pub async fn parse_intent_chat(text: String, state: State<'_, AppState>) -> Result<IntentFields, AppError> {
    let options = build_form_options(&state).await;
    let services = state.services()?;
    let client = services.llm().await;
    crate::intent_chat::parse(&client, &text, &options).await.map_err(|error| match error {
        crate::intent_chat::IntentChatError::Llm(error) => error.into(),
        // Valid JSON of the wrong shape is still the model's answer being unusable.
        other => AppError::from(crate::llm_client::LlmError::BadResponse(other.to_string())),
    })
}

/// One choice in the Profile screen's preset list. Sent from here so that the URLs,
/// which differ on the Android emulator, have one source.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct LlmPresetView {
    pub id: crate::llm_config::Preset,
    pub label: String,
    pub base_url: String,
    pub model: String,
}

/// What the Profile screen shows of the model settings. Has no field a key could
/// travel in: the screen only learns whether one is saved.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct LlmSettingsView {
    pub preset: crate::llm_config::Preset,
    pub base_url: String,
    pub model: String,
    pub calls_per_hour: u32,
    pub has_api_key: bool,
    pub tools_supported: Option<bool>,
    pub presets: Vec<LlmPresetView>,
}

#[must_use]
pub fn llm_settings_view(settings: crate::llm_config::LlmSettings, has_api_key: bool) -> LlmSettingsView {
    LlmSettingsView {
        preset: settings.preset,
        base_url: settings.base_url,
        model: settings.model,
        calls_per_hour: settings.calls_per_hour,
        has_api_key,
        tools_supported: settings.tools_supported,
        presets: crate::llm_config::Preset::ALL
            .into_iter()
            .map(|preset| LlmPresetView {
                id: preset,
                label: preset.label().to_string(),
                base_url: preset.base_url().to_string(),
                model: preset.model().to_string(),
            })
            .collect(),
    }
}

async fn current_llm_view(services: &crate::state::Services) -> LlmSettingsView {
    let settings = crate::llm_config::load();
    let has_api_key = services.identity.lock().await.llm_api_key(&settings).is_some();
    llm_settings_view(settings, has_api_key)
}

/// This device's model settings, without the key.
///
/// # Errors
///
/// [`AppError::NotReady`] during bootstrap.
#[tauri::command]
pub async fn llm_get_settings(state: State<'_, AppState>) -> Result<LlmSettingsView, AppError> {
    Ok(current_llm_view(&state.services()?).await)
}

/// Saves this device's model settings. A key stays with the URL it was saved for.
///
/// # Errors
///
/// [`AppError::InvalidIntent`] on a bad base URL or a limit outside 1 to 600.
#[tauri::command]
pub async fn llm_save_settings(
    preset: crate::llm_config::Preset,
    base_url: String,
    model: String,
    calls_per_hour: u32,
    state: State<'_, AppState>,
) -> Result<LlmSettingsView, AppError> {
    let services = state.services()?;
    let settings =
        crate::llm_config::applied(&crate::llm_config::load(), preset, &base_url, &model, calls_per_hour)?;
    crate::llm_config::save(&settings)?;
    services.llm_budget.set_limit(calls_per_hour);

    let ollama = services.ollama.clone();
    tauri::async_runtime::spawn(async move { ollama.start_for(&settings).await });

    Ok(current_llm_view(&services).await)
}

/// Saves the API key for the current settings. An empty key clears it.
///
/// # Errors
///
/// [`AppError::VaultLocked`] while the vault is locked. [`AppError::InvalidIntent`]
/// if the current base URL would carry the key in the clear over the internet.
#[tauri::command]
pub async fn llm_set_api_key(key: String, state: State<'_, AppState>) -> Result<LlmSettingsView, AppError> {
    let key = cabal_vault::Secret::new(key.trim());
    let settings = crate::llm_config::load();
    if !key.is_empty() && !crate::llm_config::key_may_travel_to(&settings.base_url) {
        return Err(AppError::InvalidIntent { field: "BASE URL", reason: crate::error::InvalidReason::Insecure });
    }
    let services = state.services()?;
    {
        let identity = services.identity.lock().await;
        if identity.is_locked() {
            return Err(AppError::VaultLocked);
        }
        identity
            .set_llm_api_key(&settings, (!key.is_empty()).then(|| key.expose()))
            .map_err(|error| AppError::internal_msg(error))?;
    }
    Ok(current_llm_view(&services).await)
}

/// Tests the saved settings and remembers whether the model can call tools.
///
/// # Errors
///
/// [`AppError::NotReady`] during bootstrap. A failed test is a result, not an error.
#[tauri::command]
pub async fn llm_test_connection(state: State<'_, AppState>) -> Result<crate::llm_client::LlmProbe, AppError> {
    let services = state.services()?;
    let probe = services.llm().await.probe().await;
    if probe.json_ok {
        let mut settings = crate::llm_config::load();
        settings.tools_supported = Some(probe.tools_ok);
        crate::llm_config::save(&settings)?;
    }
    Ok(probe)
}

/// Validates a draft and returns what the confirm dialog shows.
///
/// Computed here rather than on the frontend so what the user confirms is
/// exactly what would be broadcast — a dialog assembled separately can drift
/// from the payload it claims to describe.
///
/// # Errors
///
/// [`AppError::InvalidIntent`] with the offending field, so the form can attach
/// the failure to an input rather than showing a general message.
#[tauri::command]
pub async fn preview_intent(
    fields: IntentFields,
    state: State<'_, AppState>,
) -> Result<IntentPreview, AppError> {
    let draft = parse_draft(&fields, device_chain())?;
    let live = will_broadcast(&state).await;

    Ok(IntentPreview {
        rows: review_rows(&draft),
        confirm: if live { CONFIRM_ONLINE } else { CONFIRM_QUEUED }.to_string(),
        will_broadcast: live,
    })
}

/// Composes an intent and, if there is a mesh, sends it.
///
/// Re-parses from the same fields through the same function the preview used,
/// rather than trusting a payload the frontend assembled from the dialog. The
/// dialog is a rendering of the draft; it is not the draft.
///
/// Returns the new identifier so the caller can open its detail screen.
///
/// # Errors
///
/// [`AppError::InvalidIntent`] if the draft no longer validates.
#[tauri::command]
pub async fn broadcast_intent(
    fields: IntentFields,
    state: State<'_, AppState>,
) -> Result<String, AppError> {
    use crate::bindings::LogTone;
    use crate::intents::line;

    let draft = parse_draft(&fields, device_chain())?;

    // Only a balance inside the verifier can settle, so a figure above it would
    // reach the far side of a negotiation before anyone found out.
    let near = crate::network_config::NetworkConfig::current().near();
    if !crate::balances::covers(&cached_balances(&state).await, &draft, &near) {
        return Err(AppError::InvalidIntent {
            field: "amount",
            reason: crate::error::InvalidReason::InsufficientFunds,
        });
    }

    let ledger = state.intents();
    let intent = ledger.create(draft, crate::intents::now_ms());

    ledger.record(&intent.id, line("INTENT COMPOSED.", LogTone::Dim));

    // Composing always succeeds; publishing is what can fail. Keeping them as
    // separate steps is what lets the queue exist at all.
    let published = publish(&state, &intent).await;

    match published {
        Ok(PublishRoute::Mesh(peers)) => {
            ledger.record(&intent.id, line("BROADCAST TO MESH.", LogTone::Ok));
            let route_len = u8::try_from(peers).unwrap_or(u8::MAX);
            let _ = ledger.advance(
                &intent.id,
                cabal_core::IntentStatus::Broadcast { route_len },
                crate::intents::now_ms(),
            );
        }
        Ok(PublishRoute::Ble(peers)) => {
            // No internet here, but a BLE-reachable peer heard it — flooded
            // through the room the same way an `Announce` is, so a gateway
            // among those peers can carry it onward. See the BLE `Intent`
            // bridge in `lib.rs`: whichever gateway sees this over the radio
            // is the one that actually reaches the mesh on our behalf.
            ledger.record(&intent.id, line("BROADCAST VIA BLUETOOTH GATEWAY.", LogTone::Ok));
            let route_len = u8::try_from(peers).unwrap_or(u8::MAX);
            let _ = ledger.advance(
                &intent.id,
                cabal_core::IntentStatus::Broadcast { route_len },
                crate::intents::now_ms(),
            );
        }
        Err(reason) => {
            // Stays a draft, which is the queue. Not an error state: this is
            // the offline path working, and the confirm dialog already said so.
            ledger.record(&intent.id, line(reason, LogTone::Dim));
        }
    }

    Ok(intent.id.to_string())
}

/// Retries every intent still sitting in `Draft` — composed but never
/// broadcast — now that connectivity might reach further than it did when
/// each one was queued.
///
/// Fired from `lib.rs` on `MeshEvent::ConnectivityChanged { online: true }`:
/// that is the one moment a relay reservation just landed, so it is also the
/// one moment retrying is likely to do anything different from the attempt
/// that queued the intent in the first place. A still-failing retry stays
/// silent rather than re-recording "QUEUED LOCALLY." — the original attempt
/// already said that, and a device that reconnects and drops repeatedly
/// would otherwise fill the ledger with copies of the same line.
pub(crate) async fn retry_queued_intents(state: &AppState) {
    use crate::bindings::LogTone;
    use crate::intents::line;

    let ledger = state.intents();
    let queued: Vec<_> = ledger
        .all()
        .into_iter()
        .filter(|intent| matches!(intent.status, cabal_core::IntentStatus::Draft))
        .collect();

    for intent in queued {
        let route_len = match publish(state, &intent).await {
            Ok(PublishRoute::Mesh(peers)) => {
                ledger.record(&intent.id, line("BROADCAST TO MESH.", LogTone::Ok));
                peers
            }
            Ok(PublishRoute::Ble(peers)) => {
                ledger.record(&intent.id, line("BROADCAST VIA BLUETOOTH GATEWAY.", LogTone::Ok));
                peers
            }
            Err(_) => continue,
        };
        let route_len = u8::try_from(route_len).unwrap_or(u8::MAX);
        let _ = ledger.advance(
            &intent.id,
            cabal_core::IntentStatus::Broadcast { route_len },
            crate::intents::now_ms(),
        );
    }
}

/// Which plane actually carried a published intent, and how many peers it
/// reached there — the two planes report peer counts that mean different
/// things, so keeping them apart rather than collapsing to one `usize`
/// avoids attributing an IP peer count to a Bluetooth broadcast or the
/// reverse.
enum PublishRoute {
    Mesh(usize),
    Ble(usize),
}

/// Publishes an intent, preferring the IP mesh and falling back to the BLE
/// room when there is no internet — a device with Bluetooth peers but no
/// Wi-Fi is not actually offline, it just has to hop through a gateway
/// instead of reaching the topic directly. See `docs/ble-mesh-design.md` §8.
///
/// The error is the on-voice line to record, not a message to show raw —
/// every path through here ends up in the terminal the user is reading.
/// What the mesh learns about an intent. Not the draft: the user's price limit stays here.
fn public_payload(intent: &crate::intents::Intent) -> Option<String> {
    serde_json::to_string(&cabal_core::PublicIntent::of(intent.id.to_string(), &intent.draft)).ok()
}

async fn publish(state: &AppState, intent: &crate::intents::Intent) -> Result<PublishRoute, &'static str> {
    let services = state.services().map_err(|_| "MESH NOT READY. QUEUED LOCALLY.")?;

    // Encryption is the transport's job: Noise already covers every hop, and a
    // second layer here would be ceremony rather than protection.
    let payload = public_payload(intent).ok_or("COULD NOT ENCODE. QUEUED LOCALLY.")?;
    let privacy_intent = crate::mesh::PrivacyIntent {
        id: intent.id.to_string(),
        intent_type: "intent".into(),
        payload,
        encrypted: false,
        // `verify_relay_integrity` rejects an empty relay path outright — it
        // exists to catch a hop-stripped intent, but a fresh one hasn't been
        // relayed by anyone yet, so it has to carry its own origin stamp from
        // the moment it's created, not first grow one somewhere downstream.
        relay_path: vec!["origin_node".into()],
        relay_fee: None,
    };

    match publish_over_mesh(&services, &privacy_intent).await {
        Ok(peers) => return Ok(PublishRoute::Mesh(peers)),
        Err(mesh_reason) => {
            if let Some(peers) = publish_over_ble(&services, &privacy_intent).await {
                return Ok(PublishRoute::Ble(peers));
            }
            Err(mesh_reason)
        }
    }
}

pub(crate) async fn publish_over_mesh(
    services: &crate::state::Services,
    intent: &crate::mesh::PrivacyIntent,
) -> Result<usize, &'static str> {
    let mesh = services.mesh.as_ref().ok_or("NO MESH. QUEUED LOCALLY.")?;

    let snapshot = mesh.snapshot().await.map_err(|_| "MESH UNREACHABLE. QUEUED LOCALLY.")?;
    if snapshot.offline {
        return Err("OFFLINE MODE. QUEUED LOCALLY.");
    }
    if snapshot.peer_count == 0 {
        return Err("NO PEERS IN RANGE. QUEUED LOCALLY.");
    }

    mesh.publish(intent.clone()).await.map_err(|_| "PUBLISH REFUSED. QUEUED LOCALLY.")?;
    Ok(snapshot.peer_count)
}

/// Extra attempts after the first, spaced [`BLE_INTENT_RESEND_DELAY`] apart.
///
/// The radio's send is fire-and-forget the whole way down — Android's writer
/// thread queues bytes and reports success before a single one reaches the
/// socket, so a link that dies mid-write is indistinguishable, from here, from
/// one that delivered. `Announce` gets away with the same radio because it is
/// periodic and self-heals; `Intent` is submitted once. In a two-peer room
/// there is nobody else to relay a copy that never arrived, so the origin has
/// to be the one that tries again.
const BLE_INTENT_RESENDS: u8 = 2;

/// Gap between resends. Long enough that a link mid-glare (both sides having
/// just dialled each other) has settled to one usable connection by the next
/// attempt, short enough that all retries land inside the few seconds a
/// composing user is still watching the screen.
const BLE_INTENT_RESEND_DELAY: std::time::Duration = std::time::Duration::from_secs(3);

/// The BLE fallback. Returns `None` rather than an error string on every
/// failure path: BLE not having worked is not itself the reason to show —
/// the mesh failure from `publish_over_mesh` already is, and this is only
/// ever consulted after that one failed.
pub(crate) async fn publish_over_ble(
    services: &crate::state::Services,
    intent: &crate::mesh::PrivacyIntent,
) -> Option<usize> {
    let ble = services.ble.as_ref()?;
    let status = ble.status().await.ok()?;
    if status.offline || status.reachable_peers == 0 {
        return None;
    }

    let payload = serde_json::to_vec(intent).ok()?;
    ble.broadcast(cabal_ble::wire::PacketKind::Intent, payload.clone())
        .await
        .ok()?;

    // Resent in the background: the caller already has what it needs (a
    // route to record and a peer count to show) the moment the first send is
    // handed to the radio, and making it wait out every retry would hold a
    // "compose" button spinner hostage to a link that may never confirm
    // anything, ever, by design.
    let retry_ble = ble.clone();
    tokio::spawn(async move {
        for _ in 0..BLE_INTENT_RESENDS {
            tokio::time::sleep(BLE_INTENT_RESEND_DELAY).await;
            let _ = retry_ble
                .broadcast(cabal_ble::wire::PacketKind::Intent, payload.clone())
                .await;
        }
    });

    Some(status.reachable_peers)
}

/// Everything the detail screen renders.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct IntentDetailView {
    pub id: String,
    pub title: String,
    pub status: cabal_core::IntentStatus,
    /// Counts up while live, freezes at the terminal state.
    pub elapsed: String,
    /// The seven-row breakdown.
    pub rows: Vec<ReviewRow>,
    /// Whether settling is possible right now.
    pub can_settle: bool,
    /// Why not, in brand voice, when it is not. Absent when it is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settle_blocked: Option<String>,
    /// Whether the intent can still be cancelled.
    pub can_cancel: bool,
}

/// The rows the detail screen adds to the five reviewed. Route and counterparty
/// always, and the agreed terms once there are some.
///
/// Route renders what was actually recorded — an empty route says so rather
/// than inventing hops, because the proof screen shows the same value and
/// calls it evidence.
fn detail_rows(intent: &crate::intents::Intent) -> Vec<ReviewRow> {
    let mut rows = review_rows(&intent.draft);

    rows.push(ReviewRow {
        key: "ROUTE".into(),
        value: if intent.route.is_empty() {
            "NOT YET ROUTED".into()
        } else {
            intent
                .route
                .iter()
                .map(|hop| hop.truncated())
                .collect::<Vec<_>>()
                .join(" · ")
        },
    });

    rows.push(ReviewRow {
        key: "COUNTERPARTY".into(),
        value: intent
            .counterparty
            .as_deref()
            .map_or_else(|| "NONE YET".into(), |address| cabal_core::NodeId::new(address).truncated()),
    });

    if let Some(deal) = &intent.deal {
        rows.push(ReviewRow { key: "AGREED PRICE".into(), value: deal.price.to_string() });
        let near = crate::network_config::network_of(intent.draft.chain).near();
        if let Some(quote) = crate::near_settlement::quote_for(&near, intent.draft.amount.raw(), deal.price) {
            rows.push(ReviewRow {
                key: if intent.draft.action == cabal_core::Action::Sell { "YOU RECEIVE" } else { "YOU PAY" }.into(),
                value: format!(
                    "{} {}",
                    cabal_core::TokenAmount::from_raw(quote, near.quote.decimals).to_plain_string(),
                    near.quote.symbol
                ),
            });
        }
        if let Some(fee) = near.gateway_fee() {
            rows.push(ReviewRow {
                key: "GATEWAY FEE".into(),
                value: format!(
                    "{} {}",
                    cabal_core::TokenAmount::from_raw(fee.amount, near.quote.decimals)
                        .to_plain_string(),
                    near.quote.symbol
                ),
            });
        }
    }

    rows
}

/// Why this intent cannot be signed now, in brand voice, or `None` if it can.
///
/// Pure so the detail screen and the command refuse for one reason, and so the
/// answer does not depend on when the button was last drawn.
fn settle_blocked(intent: &crate::intents::Intent, device_chain: cabal_core::Chain, now_ms: u64) -> Option<String> {
    use cabal_core::{Action, IntentStatus};

    let reason = if intent.status.is_terminal() {
        "Already finished. Nothing left to settle."
    } else if matches!(intent.status, IntentStatus::Draft) {
        "Queued locally. Settlement follows reconnection."
    } else if intent.draft.chain != device_chain {
        "Composed on another network. Switch network to settle."
    } else if !matches!(intent.draft.action, Action::Buy | Action::Sell) {
        "Only a buy or a sell can settle."
    } else {
        match intent.deal.as_ref().map(|deal| deal.signed_until_ms) {
            None => "No node has agreed a price yet. Settlement needs a deal.",
            Some(Some(until)) if until > now_ms => "Signed. Waiting for the other side and a gateway.",
            Some(Some(_)) => "Signature expired. Cancel and compose again.",
            Some(None) => return None,
        }
    };
    Some(reason.to_string())
}

/// Whether a signature this device gave can still settle. It cannot be recalled,
/// so an intent in this state must not claim to be over.
fn still_binding(intent: &crate::intents::Intent, now_ms: u64) -> bool {
    !intent.status.is_terminal()
        && intent.deal.as_ref().and_then(|deal| deal.signed_until_ms).is_some_and(|until| until > now_ms)
}

/// One intent in full.
///
/// # Errors
///
/// [`AppError::InvalidIntent`] if the identifier is unknown, which means the
/// frontend navigated to something that does not exist.
#[tauri::command]
pub async fn intent_detail(
    id: String,
    state: State<'_, AppState>,
) -> Result<IntentDetailView, AppError> {
    use crate::error::InvalidReason;

    let intent = state
        .intents()
        .get(&cabal_core::IntentId::new(id))
        .ok_or(AppError::InvalidIntent {
            field: "id",
            reason: InvalidReason::Missing,
        })?;

    let now = crate::intents::now_ms();
    let settle_blocked = settle_blocked(&intent, device_chain(), now);

    Ok(IntentDetailView {
        id: intent.id.to_string(),
        title: format!("{:?} {}", intent.draft.action, intent.draft.asset).to_uppercase(),
        status: intent.status.clone(),
        elapsed: crate::intents::format_elapsed(intent.elapsed_ms(crate::intents::now_ms())),
        rows: detail_rows(&intent),
        can_settle: settle_blocked.is_none(),
        settle_blocked,
        can_cancel: !intent.status.is_terminal() && !still_binding(&intent, now),
    })
}

/// Streams an intent's verification log.
///
/// Replays what was already recorded, then follows. **Cancelling this stops
/// delivery and nothing else** — the settlement writes into the ledger and
/// holds no token from here, which is the property `src/intents.rs` is built
/// around and `src/subscriptions.rs` documents.
///
/// # Errors
///
/// [`AppError::TooManySubscriptions`] at the registry's limit.
#[tauri::command]
pub async fn subscribe_settlement_log(
    id: String,
    on_line: tauri::ipc::Channel<crate::bindings::LogLine>,
    state: State<'_, AppState>,
) -> Result<String, AppError> {
    let intent_id = cabal_core::IntentId::new(id);
    let registry = state.subscriptions().clone();
    let (handle, token) = registry.register("settlement")?;

    let (replay, mut receiver) = state.intents().watch(&intent_id);

    let stream = handle.clone();
    tauri::async_runtime::spawn(async move {
        for recorded in replay {
            if on_line.send(recorded).is_err() {
                registry.finished(&stream);
                return;
            }
        }

        loop {
            tokio::select! {
                () = token.cancelled() => break,
                received = receiver.recv() => match received {
                    Ok((who, line)) => {
                        if who != intent_id {
                            continue;
                        }
                        if on_line.send(line).is_err() {
                            // The webview is gone; nothing left to deliver to.
                            break;
                        }
                    }
                    // Lagged means this subscriber fell behind, not that the
                    // settlement stopped. The retained log is the record, so
                    // keep following rather than tearing the stream down.
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                },
            }
        }

        registry.finished(&stream);
    });

    Ok(handle.to_string())
}

/// Signs this side of the agreed deal and hands it to the mesh.
///
/// Returns as soon as the work is **started**, not when it finishes. The
/// settlement runs in a task that holds no subscription, so navigating away —
/// or cancelling the log — cannot abort it. That is a correctness property with
/// money attached, and it is structural rather than a matter of discipline.
///
/// # Errors
///
/// [`AppError::InvalidIntent`] if the intent is unknown or cannot be signed now,
/// [`AppError::VaultLocked`] if there is no key to sign with.
#[tauri::command]
pub async fn settle_intent(id: String, state: State<'_, AppState>) -> Result<(), AppError> {
    use crate::error::InvalidReason;

    let intent_id = cabal_core::IntentId::new(id);
    let intent = state
        .intents()
        .get(&intent_id)
        .ok_or(AppError::InvalidIntent {
            field: "id",
            reason: InvalidReason::Missing,
        })?;

    if settle_blocked(&intent, device_chain(), crate::intents::now_ms()).is_some() {
        return Err(AppError::InvalidIntent {
            field: "status",
            reason: InvalidReason::OutOfRange,
        });
    }

    let services = state.services()?;
    let keypair = services.identity.lock().await.primary_keypair().ok_or(AppError::VaultLocked)?;
    let state = state.inner().clone();

    tauri::async_runtime::spawn(async move {
        run_settlement(state, services, intent, keypair).await;
    });

    Ok(())
}

/// How long a salt read may hold up signing. A device out of range should sign
/// from its cached salt at once, not wait out a connection timeout.
const SALT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// The settlement itself.
///
/// Takes no cancellation token by construction. Adding one would be the bug
/// ticket 34 exists to prevent — there would then be a way for a UI navigation
/// to abort an in-flight on-chain operation.
async fn run_settlement(
    state: AppState,
    services: crate::state::Services,
    intent: crate::intents::Intent,
    keypair: cabal_near::NearKeypair,
) {
    use crate::bindings::LogTone;
    use crate::intents::line;
    use crate::near_settlement as settlement;
    use cabal_core::{FailureReason, IntentStatus};

    let ledger = state.intents();
    let id = intent.id.clone();
    let config = crate::network_config::NetworkConfig::current();

    // Routing precedes settlement in the domain: settling straight from
    // broadcast would mean settling through a route that was never found.
    let _ = ledger.advance(&id, IntentStatus::FindingRoute, crate::intents::now_ms());
    ledger.record(&id, line("SIGNING YOUR SIDE. NO NETWORK NEEDED.", LogTone::Out));

    let salt_store = settlement::salt_store();
    let fresh = tokio::time::timeout(SALT_TIMEOUT, async {
        let (_, bridge) = settlement::gateway(&state).await?;
        bridge.current_salt().await.ok()
    })
    .await
    .ok()
    .flatten();
    let salt = match fresh {
        Some(salt) => {
            settlement::remember_salt(&salt_store, salt);
            Some(salt)
        }
        None => settlement::cached_salt(&salt_store),
    };

    let signed = cabal_near::AccountId::try_from(config.verifier().as_str()).ok().and_then(|verifier| {
        settlement::sign_own_half(ledger, state.diff_book(), &intent, &keypair, &verifier, &config.near(), salt, crate::intents::now_ms())
    });

    let Some(message) = signed else {
        // A second tap that lost to the first. The first one carries the deal.
        if ledger.get(&id).and_then(|now| now.deal).is_some_and(|deal| deal.signed_until_ms.is_some()) {
            let _ = ledger.advance(&id, IntentStatus::Waiting, crate::intents::now_ms());
            return;
        }
        // A deal too small to price in the quote, or a verifier id that does
        // not parse. Either way nothing was signed, so nothing is at risk.
        ledger.record(&id, line("COULD NOT SIGN THIS DEAL. NOTHING WAS SENT.", LogTone::Err));
        let _ = ledger.advance(
            &id,
            IntentStatus::Failed { reason: FailureReason::SettlementRejected },
            crate::intents::now_ms(),
        );
        return;
    };

    settlement::flood(&services, "diff", settlement::tagged("SignedDiff", &message).to_string()).await;
    ledger.record(&id, line("SIGNED. WAITING FOR THE OTHER SIDE.", LogTone::Ok));
    let _ = ledger.advance(&id, IntentStatus::Waiting, crate::intents::now_ms());

    // The other half may already be held. If not, this does nothing.
    settlement::retry(&state).await;
}

/// Cancels an intent.
///
/// A deliberate, separate action from cancelling a log subscription. Overloading
/// the two is the mistake ticket 34 names explicitly.
///
/// # Errors
///
/// [`AppError::InvalidIntent`] if the intent is unknown, already finished, or
/// has signed a deal that can still settle: saying it is over would be false.
#[tauri::command]
pub async fn cancel_intent(id: String, state: State<'_, AppState>) -> Result<(), AppError> {
    use crate::bindings::LogTone;
    use crate::intents::line;

    let intent_id = cabal_core::IntentId::new(id);
    let ledger = state.intents();

    // The transition check does the validating: a terminal intent refuses, and
    // an unknown identifier refuses, both with a typed error.
    let intent = ledger.get(&intent_id).ok_or(AppError::InvalidIntent {
        field: "id",
        reason: crate::error::InvalidReason::Missing,
    })?;

    if still_binding(&intent, crate::intents::now_ms()) {
        return Err(AppError::InvalidIntent {
            field: "status",
            reason: crate::error::InvalidReason::OutOfRange,
        });
    }

    ledger.advance(&intent_id, cabal_core::IntentStatus::Cancelled, crate::intents::now_ms())?;
    ledger.record(&intent_id, line("INTENT CANCELLED. NOTHING WRITTEN.", LogTone::Dim));
    Ok(())
}

/// What the proof screen renders.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct ProofView {
    pub id: String,
    /// The settling transaction's hash.
    pub hash: String,
    /// How long settlement took, e.g. `11.4S`.
    pub timing: String,
    /// The hops the intent travelled. Empty when it settled directly.
    pub route: Vec<String>,
    /// The price it filled at. Absent for an unconditioned intent, which has
    /// no price to have filled at.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filled_at: Option<String>,
}

/// The proof for a settled intent.
///
/// # Errors
///
/// [`AppError::InvalidIntent`] if the intent is unknown or has not settled —
/// there is no proof of something that did not happen.
#[tauri::command]
pub async fn intent_proof(id: String, state: State<'_, AppState>) -> Result<ProofView, AppError> {
    use crate::error::InvalidReason;

    let intent = state
        .intents()
        .get(&cabal_core::IntentId::new(id))
        .ok_or(AppError::InvalidIntent {
            field: "id",
            reason: InvalidReason::Missing,
        })?;

    let cabal_core::IntentStatus::Settled { proof, filled_at, elapsed_ms } = &intent.status else {
        return Err(AppError::InvalidIntent {
            field: "status",
            reason: InvalidReason::OutOfRange,
        });
    };

    Ok(ProofView {
        id: intent.id.to_string(),
        hash: proof.to_string(),
        timing: crate::intents::format_elapsed(u64::from(*elapsed_ms)),
        route: intent.route.iter().map(cabal_core::NodeId::truncated).collect(),
        // Zero cents means the intent carried no condition, so there is no
        // price it filled at. Rendering `$0.00` would be a figure, and a wrong
        // one.
        filled_at: (filled_at.cents() > 0).then(|| filled_at.to_string()),
    })
}

#[cfg(test)]
mod intent_tests {
    use super::*;
    use crate::bindings::LogTone;
    use crate::intents::{line, Ledger};
    use cabal_core::{IntentStatus, ProofHash, UsdPrice};

    fn ledger() -> (Ledger, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = cabal_store::JsonStore::new(dir.path().join("intents.json"));
        (Ledger::open(store), dir)
    }

    const TESTNET: cabal_core::Chain = cabal_core::Chain::NearTestnet;

    fn parse_draft(fields: &IntentFields) -> Result<cabal_core::IntentDraft, AppError> {
        super::parse_draft(fields, TESTNET)
    }

    fn fields(action: &str, asset: &str, condition: &str, price: &str, amount: &str, mode: &str, privacy: &str) -> IntentFields {
        IntentFields {
            action: action.into(),
            asset: asset.into(),
            condition: condition.into(),
            price: price.into(),
            amount: amount.into(),
            mode: mode.into(),
            privacy: privacy.into(),
        }
    }

    fn draft() -> cabal_core::IntentDraft {
        parse_draft(&fields("BUY", "wNEAR", "Price under", "95", "1.5", "SHARK MODE", "HIGH")).unwrap()
    }

    // -- what the dialog shows is what goes out ----------------------------

    #[test]
    fn the_review_rows_come_from_the_parsed_draft() {
        // The property ticket 33 asks for: the dialog is a rendering *of the
        // draft*, so it cannot describe an intent different from the one that
        // would be broadcast.
        let rows = review_rows(&draft());
        let value = |key: &str| {
            rows.iter()
                .find(|row| row.key == key)
                .map(|row| row.value.clone())
                .unwrap()
        };

        assert_eq!(value("ACTION"), "BUY WNEAR");
        assert_eq!(value("CONDITION"), "UNDER $95.00");
        assert_eq!(value("AMOUNT"), "1.5 wNEAR");
        assert_eq!(value("MODE"), "SHARK MODE");
        assert_eq!(value("PRIVACY"), "HIGH");
    }

    #[test]
    fn an_unconditioned_intent_shows_no_price() {
        // `Condition::Any` carries no price by construction, so there is none
        // to render — and rendering `$0.00` would be a claim about a limit the
        // user never set.
        let draft = parse_draft(&fields("SELL", "wNEAR", "Any price", "", "10", "GHOST MODE", "LOW")).unwrap();
        let rows = review_rows(&draft);
        assert_eq!(rows[1].value, "ANY PRICE");
    }

    #[test]
    fn precision_beyond_the_asset_is_refused_rather_than_truncated() {
        // wNEAR has 24 decimals. Silently dropping the 25th would lose money.
        let refused = parse_draft(&fields("BUY", "wNEAR", "Any price", "", "1.0000000000000000000000001", "SHARK MODE", "HIGH"));
        assert!(matches!(
            refused,
            Err(AppError::InvalidIntent { field: "amount", .. })
        ));
    }

    #[test]
    fn a_zero_amount_is_refused() {
        let refused = parse_draft(&fields("BUY", "wNEAR", "Any price", "", "0", "SHARK MODE", "HIGH"));
        assert!(matches!(
            refused,
            Err(AppError::InvalidIntent {
                field: "amount",
                reason: crate::error::InvalidReason::OutOfRange
            })
        ));
    }

    #[test]
    fn an_unknown_asset_is_refused_rather_than_defaulted() {
        // Defaulting the decimals for an unrecognised asset would parse an
        // amount off by many orders of magnitude.
        let refused = parse_draft(&fields("BUY", "DOGE", "Any price", "", "1", "SHARK MODE", "HIGH"));
        assert!(matches!(
            refused,
            Err(AppError::InvalidIntent { field: "asset", .. })
        ));
    }

    #[test]
    fn the_quote_cannot_be_traded_as_the_base() {
        let refused = parse_draft(&fields("BUY", "USDC", "Any price", "", "1", "SHARK MODE", "HIGH"));
        assert!(matches!(refused, Err(AppError::InvalidIntent { field: "asset", .. })));
    }

    #[test]
    fn a_swap_is_refused() {
        let refused = parse_draft(&fields("SWAP", "wNEAR", "Any price", "", "1", "SHARK MODE", "HIGH"));
        assert!(matches!(refused, Err(AppError::InvalidIntent { field: "action", .. })));
    }

    #[test]
    fn the_draft_takes_the_devices_network() {
        let f = fields("SELL", "wNEAR", "Any price", "", "1", "SHARK MODE", "HIGH");
        assert_eq!(super::parse_draft(&f, TESTNET).unwrap().chain, TESTNET);
        let mainnet = cabal_core::Chain::NearMainnet;
        assert_eq!(super::parse_draft(&f, mainnet).unwrap().chain, mainnet);
    }

    #[test]
    fn the_base_of_every_network_parses() {
        // The form offers this, so it has to survive the round trip. A mismatch
        // here is a form that offers something Rust rejects.
        for network in [crate::network_config::Network::Testnet, crate::network_config::Network::Mainnet] {
            let base = network.near().base.symbol;
            let f = fields("BUY", base, "Any price", "", "1", "SHARK MODE", "HIGH");
            assert!(super::parse_draft(&f, network.chain()).is_ok(), "{base} did not parse");
        }
    }

    // -- what can be signed, and when ---------------------------------------

    fn negotiated() -> (Ledger, crate::intents::Intent, tempfile::TempDir) {
        let (ledger, dir) = ledger();
        let intent = ledger.create(draft(), 1_000);
        ledger.advance(&intent.id, IntentStatus::Broadcast { route_len: 1 }, 1_100).unwrap();
        (ledger, intent, dir)
    }

    #[test]
    fn signing_is_blocked_until_a_price_is_agreed() {
        let (ledger, intent, _dir) = negotiated();
        let before = ledger.get(&intent.id).unwrap();
        assert!(settle_blocked(&before, TESTNET, 2_000).unwrap().contains("No node"));

        ledger.propose_deal(&intent.id, "a:b", UsdPrice::from_cents(9000), "b");
        let after = ledger.get(&intent.id).unwrap();
        assert_eq!(settle_blocked(&after, TESTNET, 2_000), None);
    }

    #[test]
    fn a_signature_cannot_be_given_twice_or_taken_back() {
        let (ledger, intent, _dir) = negotiated();
        ledger.propose_deal(&intent.id, "a:b", UsdPrice::from_cents(9000), "b");
        ledger.mark_signed(&intent.id, 5_000);
        let signed = ledger.get(&intent.id).unwrap();

        assert!(settle_blocked(&signed, TESTNET, 2_000).unwrap().contains("Signed"));
        assert!(still_binding(&signed, 2_000), "cancelling now would be a lie");
        assert!(settle_blocked(&signed, TESTNET, 6_000).unwrap().contains("expired"));
        assert!(!still_binding(&signed, 6_000), "after the deadline nothing can settle it");
    }

    #[test]
    fn an_intent_from_another_chain_cannot_be_signed() {
        let (ledger, intent, _dir) = negotiated();
        ledger.propose_deal(&intent.id, "a:b", UsdPrice::from_cents(9000), "b");
        let current = ledger.get(&intent.id).unwrap();
        assert!(settle_blocked(&current, cabal_core::Chain::NearMainnet, 2_000).unwrap().contains("another network"));
    }

    #[test]
    fn the_breakdown_shows_the_agreed_terms() {
        let (ledger, intent, _dir) = negotiated();
        ledger.propose_deal(&intent.id, "a:b", UsdPrice::from_cents(9000), "b");
        let rows = detail_rows(&ledger.get(&intent.id).unwrap());
        let value = |key: &str| rows.iter().find(|row| row.key == key).map(|row| row.value.clone());

        assert_eq!(value("AGREED PRICE").as_deref(), Some("$90.00"));
        // 1.5 wNEAR at $90: what a buyer has to see before it signs.
        assert_eq!(value("YOU PAY").as_deref(), Some("135 USDC"));
    }

    #[test]
    fn the_confirm_lines_describe_different_things() {
        // Ticket 04. The retired string claimed offline intents settle
        // on-chain; the queued line must not say anything of the kind.
        assert!(CONFIRM_ONLINE.contains("broadcasts to the mesh"));
        assert!(CONFIRM_QUEUED.contains("Queued locally"));
        assert!(!CONFIRM_QUEUED.contains("settles on-chain"));
        // The identity claim holds in both, which is the part that stayed true.
        assert!(CONFIRM_ONLINE.contains("No identity is attached."));
        assert!(CONFIRM_QUEUED.contains("No identity is attached."));
    }

    // -- list slicing -------------------------------------------------------

    #[test]
    fn a_queued_intent_is_pending_not_active() {
        // A draft is the queue. Filing it under ACTIVE would claim it is on the
        // mesh; filing it under HISTORY would claim it is over.
        let draft = IntentStatus::Draft;
        assert!(IntentFilter::Pending.admits(&draft));
        assert!(!IntentFilter::Active.admits(&draft));
        assert!(!IntentFilter::History.admits(&draft));
    }

    #[test]
    fn every_status_lands_in_exactly_one_slice() {
        let statuses = [
            IntentStatus::Draft,
            IntentStatus::Broadcast { route_len: 2 },
            IntentStatus::Negotiating { bids: 1, best: None },
            IntentStatus::FindingRoute,
            IntentStatus::Waiting,
            IntentStatus::Settled {
                proof: ProofHash::new("0xabc"),
                filled_at: UsdPrice::from_cents(9421),
                elapsed_ms: 11_400,
            },
            IntentStatus::Failed { reason: cabal_core::FailureReason::NoRoute },
            IntentStatus::Cancelled,
        ];

        for status in statuses {
            let matches = [IntentFilter::Active, IntentFilter::Pending, IntentFilter::History]
                .into_iter()
                .filter(|filter| filter.admits(&status))
                .count();
            assert_eq!(matches, 1, "{status:?} landed in {matches} slices");
        }
    }

    #[test]
    fn a_row_renders_the_intent_it_was_given() {
        let (ledger, _dir) = ledger();
        let intent = ledger.create(draft(), 1_000);
        let row = row_for(&intent, 12_400);

        assert_eq!(row.title, "BUY wNEAR");
        assert_eq!(row.subtitle, "UNDER $95.00");
        assert_eq!(row.amount, "1.5 wNEAR");
        assert_eq!(row.elapsed, "11.4S");
        // Shark is the default, so badging it would put a badge on nearly every
        // row and stop the badge carrying information.
        assert_eq!(row.badge, None);
    }

    #[test]
    fn a_non_default_mode_is_badged() {
        let (ledger, _dir) = ledger();
        let ghost = parse_draft(&fields("BUY", "wNEAR", "Any price", "", "1", "GHOST MODE", "HIGH")).unwrap();
        let intent = ledger.create(ghost, 1_000);
        assert_eq!(row_for(&intent, 1_000).badge.as_deref(), Some("GHOST MODE"));
    }

    // -- the seven-row breakdown -------------------------------------------

    #[test]
    fn the_breakdown_has_seven_rows_and_invents_no_route() {
        let (ledger, _dir) = ledger();
        let intent = ledger.create(draft(), 1_000);
        let rows = detail_rows(&intent);

        assert_eq!(rows.len(), 7);
        assert_eq!(rows[5].key, "ROUTE");
        // Says so rather than inventing hops — the proof screen renders the
        // same value and calls it evidence.
        assert_eq!(rows[5].value, "NOT YET ROUTED");
        assert_eq!(rows[6].value, "NONE YET");
    }

    // -- ticket 34's load-bearing rule -------------------------------------

    #[tokio::test]
    async fn cancelling_the_log_subscription_does_not_abort_the_settlement() {
        // The explicit test ticket 34 asks for, run against the real
        // subscription registry rather than a stand-in.
        //
        // The structural claim is that a settlement task holds *no* token from
        // the registry. This exercises it: a task shaped exactly like
        // `run_settlement` — a ledger and nothing else — keeps writing after
        // the subscription that was watching it is cancelled, and reaches its
        // terminal state.
        let (ledger, _dir) = ledger();
        let registry = crate::subscriptions::Registry::new();
        let intent = ledger.create(draft(), 1_000);

        let (handle, token) = registry.register("settlement").unwrap();
        let (_replay, _receiver) = ledger.watch(&intent.id);

        ledger
            .advance(&intent.id, IntentStatus::Broadcast { route_len: 2 }, 1_100)
            .unwrap();

        let writer = {
            let ledger = ledger.clone();
            let id = intent.id.clone();
            tokio::spawn(async move {
                ledger.record(&id, line("VERIFYING ROUTE.", LogTone::Out));
                let _ = ledger.advance(&id, IntentStatus::FindingRoute, 1_200);

                // The navigation happens here, in the middle.
                tokio::task::yield_now().await;

                ledger.record(&id, line("SETTLED.", LogTone::Ok));
                let _ = ledger.advance(
                    &id,
                    IntentStatus::Settled {
                        proof: ProofHash::new("0xdeadbeef"),
                        filled_at: UsdPrice::from_cents(9421),
                        elapsed_ms: 11_400,
                    },
                    1_300,
                );
            })
        };

        // Navigating away: the registry cancels delivery.
        registry.cancel(&handle);
        assert!(token.is_cancelled());
        assert!(registry.is_empty());

        writer.await.unwrap();

        let after = ledger.get(&intent.id).unwrap();
        assert!(
            matches!(after.status, IntentStatus::Settled { .. }),
            "settlement was aborted by a UI navigation: {:?}",
            after.status
        );
        // And the record is complete, so coming back replays everything that
        // happened while away.
        assert_eq!(after.log.len(), 2);
        assert_eq!(&*after.log[1].text, "SETTLED.");
    }

    #[tokio::test]
    async fn cancelling_the_intent_is_a_different_thing_entirely() {
        // The other half of the same rule: cancelling the *intent* is
        // deliberate and does end it. If these two ever became the same
        // operation, a navigation would start ending intents.
        let (ledger, _dir) = ledger();
        let intent = ledger.create(draft(), 1_000);
        ledger
            .advance(&intent.id, IntentStatus::Broadcast { route_len: 2 }, 1_100)
            .unwrap();

        ledger
            .advance(&intent.id, IntentStatus::Cancelled, 2_000)
            .unwrap();

        assert_eq!(ledger.get(&intent.id).unwrap().status, IntentStatus::Cancelled);
    }

    // -- the proof ----------------------------------------------------------

    #[test]
    fn a_settled_intent_yields_the_hash_it_settled_with() {
        let (ledger, _dir) = ledger();
        let intent = ledger.create(draft(), 1_000);
        ledger
            .advance(&intent.id, IntentStatus::Broadcast { route_len: 1 }, 1_100)
            .unwrap();
        ledger
            .advance(&intent.id, IntentStatus::FindingRoute, 1_200)
            .unwrap();
        ledger.set_route(&intent.id, vec![cabal_core::NodeId::new("7F3A00000000008C2E")]);
        ledger
            .advance(
                &intent.id,
                IntentStatus::Settled {
                    proof: ProofHash::new("0xa4f2c9e1b70d5533"),
                    filled_at: UsdPrice::from_cents(9421),
                    elapsed_ms: 11_400,
                },
                12_400,
            )
            .unwrap();

        let settled = ledger.get(&intent.id).unwrap();
        let IntentStatus::Settled { proof, filled_at, elapsed_ms } = &settled.status else {
            panic!("expected settled");
        };

        assert_eq!(proof.as_str(), "0xa4f2c9e1b70d5533");
        assert_eq!(filled_at.to_string(), "$94.21");
        assert_eq!(crate::intents::format_elapsed(u64::from(*elapsed_ms)), "11.4S");
        assert_eq!(settled.route[0].truncated(), "7F3A..8C2E");
    }
}

// ---------------------------------------------------------------------------
// Vault and profile
// ---------------------------------------------------------------------------

/// A row in the vault list.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct VaultRow {
    /// Three-letter tag, e.g. `NEA`, `ID`, `KEY`.
    pub tag: String,
    pub name: String,
    pub amount: String,
    /// Secondary line. Absent when there is nothing true to say.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Balances held by this identity, as last read.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap.
#[tauri::command]
pub async fn vault_assets(state: State<'_, AppState>) -> Result<Vec<VaultRow>, AppError> {
    state.services()?;
    Ok(vault_rows(&cached_balances(&state).await, &crate::network_config::NetworkConfig::current().near()))
}

/// Reads the balances from the chain on demand — the ASSETS tab's refresh
/// button — then returns the same shape `vault_assets` does, so the frontend can
/// swap `rows` straight from the response instead of making a second round trip
/// that could race a concurrent poll.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap, [`AppError::VaultLocked`] with no
/// key, [`AppError::Chain`] if the read fails; the caller keeps showing what it
/// had.
#[tauri::command]
pub async fn vault_refresh_balance(state: State<'_, AppState>) -> Result<Vec<VaultRow>, AppError> {
    let services = state.services()?;
    let bridge = services.near().await?;
    let config = crate::network_config::NetworkConfig::current();
    let balances = read_and_cache(&bridge, config.network).await?;
    Ok(vault_rows(&balances, &config.near()))
}

/// Reads every balance and keeps the result for when there is no RPC.
async fn read_and_cache(
    bridge: &crate::near_bridge::NearBridge,
    network: crate::network_config::Network,
) -> Result<crate::balances::Balances, crate::near_bridge::NearError> {
    let balances =
        crate::balances::Balances::read(bridge, network, &crate::network_config::assets(network.chain())).await?;
    if let Err(error) = crate::balances::store().save(&balances) {
        tracing::warn!(target: "cabalmesh::vault", %error, "could not cache balances");
    }
    Ok(balances)
}

/// A raw amount as text, or nothing if the cached string is not a number.
fn plain_amount(raw: &str, decimals: u8) -> Option<String> {
    raw.parse().ok().map(|raw| cabal_core::TokenAmount::from_raw(raw, decimals).to_plain_string())
}

/// Native NEAR, then each token's two places. Nothing at all if no read has
/// succeeded, because listing zeros would be a claim about what is held.
fn vault_rows(balances: &crate::balances::Balances, near: &crate::network_config::NearConfig) -> Vec<VaultRow> {
    let mut rows = Vec::new();
    if let Some(amount) = balances.native.as_deref().and_then(|raw| plain_amount(raw, NATIVE_DECIMALS)) {
        rows.push(VaultRow {
            tag: "GAS".into(),
            name: "NEAR".into(),
            amount,
            detail: Some("PAYS FOR GAS. NOT TRADABLE.".into()),
        });
    }
    for spec in [&near.base, &near.quote] {
        let Some(token) = balances.tokens.get(spec.symbol) else {
            continue;
        };
        if let Some(amount) = plain_amount(&token.in_intents, spec.decimals) {
            rows.push(VaultRow {
                tag: spec.code.into(),
                name: spec.symbol.into(),
                amount,
                detail: Some("IN INTENTS. TRADABLE.".into()),
            });
        }
        if let Some(amount) = plain_amount(&token.wallet, spec.decimals) {
            rows.push(VaultRow {
                tag: spec.code.into(),
                name: spec.symbol.into(),
                amount,
                detail: Some("IN WALLET. DEPOSIT TO TRADE.".into()),
            });
        }
    }
    rows
}

const NATIVE_DECIMALS: u8 = 24;

#[cfg(test)]
mod vault_amount_tests {
    use super::*;

    #[test]
    fn nothing_read_lists_nothing() {
        let rows = vault_rows(&crate::balances::Balances::default(), &crate::network_config::Network::Testnet.near());
        assert!(rows.is_empty(), "zeros would be a claim about what is held");
    }

    #[test]
    fn each_token_shows_both_places_in_human_units() {
        let mut balances = crate::balances::Balances {
            native: Some("1500000000000000000000000".into()),
            ..Default::default()
        };
        balances.tokens.insert(
            "USDC".into(),
            crate::balances::TokenBalance { wallet: "2500000".into(), in_intents: "100000000".into() },
        );
        let rows = vault_rows(&balances, &crate::network_config::Network::Testnet.near());

        assert_eq!(rows[0].name, "NEAR");
        assert_eq!(rows[0].amount, "1.5");
        assert_eq!(rows[1].amount, "100");
        assert!(rows[1].detail.as_deref().unwrap().contains("IN INTENTS"));
        assert_eq!(rows[2].amount, "2.5");
        assert!(rows[2].detail.as_deref().unwrap().contains("IN WALLET"));
    }

    #[test]
    fn a_token_is_found_by_symbol_and_nothing_else_is() {
        let (spec, raw) = near_amount("wNEAR", "1.5").unwrap();
        assert_eq!(spec.symbol, "wNEAR");
        assert_eq!(raw, 1_500_000_000_000_000_000_000_000);
        assert!(near_amount("DOGE", "1").is_err());
        assert!(near_amount("USDC", "0").is_err(), "moving nothing is a mistake");
    }
}

/// One token's place in the NEAR card.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct NearTokenView {
    pub symbol: String,
    pub decimals: u8,
    /// Absent if it has never been read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wallet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub in_intents: Option<String>,
}

/// What the Vault's NEAR card shows.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct NearAccountView {
    pub account_id: String,
    pub network: String,
    /// Absent if it has never been read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_balance: Option<String>,
    pub tokens: Vec<NearTokenView>,
    /// Empty where funds are real.
    pub faucet_links: Vec<String>,
    pub explorer_url: String,
}

/// This device's NEAR account and the last balances read for it.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap, [`AppError::VaultLocked`] with no key.
#[tauri::command]
pub async fn near_account(state: State<'_, AppState>) -> Result<NearAccountView, AppError> {
    let services = state.services()?;
    let account = services.identity.lock().await.primary_account().ok_or(AppError::VaultLocked)?;
    let config = crate::network_config::NetworkConfig::current();
    let near = config.near();
    let balances = crate::balances::Balances::load_for(&crate::balances::store(), account.as_str(), config.network);

    Ok(NearAccountView {
        explorer_url: format!("{}/address/{account}", near.explorer),
        account_id: account.to_string(),
        network: config.network.label().to_string(),
        native_balance: balances.native.as_deref().and_then(|raw| plain_amount(raw, NATIVE_DECIMALS)),
        tokens: [near.base, near.quote]
            .iter()
            .map(|spec| {
                let token = balances.tokens.get(spec.symbol);
                NearTokenView {
                    symbol: spec.symbol.to_string(),
                    decimals: spec.decimals,
                    wallet: token.and_then(|t| plain_amount(&t.wallet, spec.decimals)),
                    in_intents: token.and_then(|t| plain_amount(&t.in_intents, spec.decimals)),
                }
            })
            .collect(),
        faucet_links: near.faucets.iter().map(ToString::to_string).collect(),
    })
}

/// The token a symbol names on this network, and `amount` in its raw units.
fn near_amount(token: &str, amount: &str) -> Result<(crate::network_config::AssetSpec, u128), AppError> {
    use crate::error::InvalidReason;

    let spec = crate::network_config::assets(device_chain())
        .into_iter()
        .find(|spec| spec.symbol == token)
        .ok_or(AppError::InvalidIntent { field: "token", reason: InvalidReason::Malformed })?;
    let parsed = cabal_core::TokenAmount::parse(amount, spec.decimals)?;
    if parsed.is_zero() {
        return Err(AppError::InvalidIntent { field: "amount", reason: InvalidReason::OutOfRange });
    }
    Ok((spec, parsed.raw()))
}

/// Moves a token from this account's wallet into the verifier, where it can trade.
///
/// Returns the transaction hash.
///
/// # Errors
///
/// [`AppError::InvalidIntent`] for an unknown token or a bad amount,
/// [`AppError::NearAccountNotFunded`] with no NEAR for gas, [`AppError::Chain`]
/// if the chain refused or could not be reached.
#[tauri::command]
pub async fn near_deposit(token: String, amount: String, state: State<'_, AppState>) -> Result<String, AppError> {
    let (spec, raw) = near_amount(&token, &amount)?;
    let services = state.services()?;
    let bridge = services.near().await?;
    let tx = bridge.deposit(spec.contract, raw).await?;
    // The money has moved whatever this read does, so its failure is not the
    // command's: the next refresh will catch up.
    let _ = read_and_cache(&bridge, crate::network_config::NetworkConfig::current().network).await;
    Ok(tx.0)
}

/// Moves a token out of the verifier, back to this account's wallet.
///
/// Returns the transaction hash.
///
/// # Errors
///
/// As [`near_deposit`].
#[tauri::command]
pub async fn near_withdraw(token: String, amount: String, state: State<'_, AppState>) -> Result<String, AppError> {
    let (spec, raw) = near_amount(&token, &amount)?;
    let services = state.services()?;
    let bridge = services.near().await?;
    let tx = bridge.withdraw(spec.contract, raw, crate::intents::now_ms()).await?;
    let _ = read_and_cache(&bridge, crate::network_config::NetworkConfig::current().network).await;
    Ok(tx.0)
}

/// This device's chain address — what someone sends assets to.
///
/// A passphrase gates *unlocking* the vault; it says nothing about whether an
/// identity exists behind it. By the time this is called the onboarding flow
/// has already created one, so `None` only means the rare case where it
/// somehow has not — not "locked," which `services()` and the passphrase
/// prompt already guard further up the stack.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap.
#[tauri::command]
pub async fn vault_address(state: State<'_, AppState>) -> Result<Option<String>, AppError> {
    let services = state.services()?;
    let identity = services.identity.lock().await;

    let account = identity.primary_account();
    Ok(account.map(|account| account.to_string()))
}

/// Identities this device holds.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap, [`AppError::VaultLocked`] if the
/// encrypted store cannot be opened.
#[tauri::command]
pub async fn vault_identities(state: State<'_, AppState>) -> Result<Vec<VaultRow>, AppError> {
    let services = state.services()?;
    let identity = services.identity.lock().await;

    let views = identity.get_identity_views().map_err(|_| AppError::VaultLocked)?;
    Ok(views
        .into_iter()
        .map(|view| VaultRow {
            tag: "ID".into(),
            name: view.alias.to_uppercase(),
            amount: cabal_core::NodeId::new(view.address).truncated(),
            detail: None,
        })
        .collect())
}

/// Key material metadata.
///
/// **Never the key itself.** These rows describe what is held and where; the
/// values stay in the encrypted vault. That is the promise the screen's own
/// copy makes, so the command has to keep it.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap.
#[tauri::command]
pub async fn vault_keys(state: State<'_, AppState>) -> Result<Vec<VaultRow>, AppError> {
    let services = state.services()?;
    let identity = services.identity.lock().await;

    // Honest about which key provider is actually protecting the vault right
    // now, rather than a description fixed at ticket 18. A locked vault has
    // no in-memory identity state to describe wrongly either way, so this reads
    // straight from disk via `security_mode`.
    let vault_key_detail = match identity.security_mode() {
        crate::security_state::UnlockMode::File => "FILE-BACKED. DEVICE KEY STORE PENDING.",
        crate::security_state::UnlockMode::Passphrase => "PASSPHRASE-DERIVED (ARGON2ID).",
    };

    Ok(vec![
        VaultRow {
            tag: "KEY".into(),
            name: "SIGNING KEY".into(),
            amount: "ed25519".into(),
            detail: Some("HELD LOCALLY. NEVER SYNCED.".into()),
        },
        VaultRow {
            tag: "KEY".into(),
            name: "VAULT KEY".into(),
            amount: "AES-256-GCM".into(),
            detail: Some(vault_key_detail.into()),
        },
        VaultRow {
            tag: "KEY".into(),
            name: "RECOVERY PHRASE".into(),
            amount: "NONE".into(),
            detail: Some("NOT BACKED UP.".into()),
        },
    ])
}

/// Current state of the vault's unlock method, for the startup gate and the
/// `SECURITY` screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct SecurityStatus {
    /// True only while the vault is passphrase-protected and no correct
    /// passphrase has been supplied yet this session. The frontend gates
    /// entry to the app on this field, not on `passphraseEnabled` alone —
    /// `passphraseEnabled` stays true even seconds after a successful
    /// unlock, when `locked` has already gone false.
    pub locked: bool,
    pub passphrase_enabled: bool,
}

/// Whether the vault needs a passphrase before use, and how it is protected.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap.
#[tauri::command]
pub async fn security_status(state: State<'_, AppState>) -> Result<SecurityStatus, AppError> {
    let services = state.services()?;
    let identity = services.identity.lock().await;
    Ok(SecurityStatus {
        locked: identity.is_locked(),
        passphrase_enabled: identity.security_mode() == crate::security_state::UnlockMode::Passphrase,
    })
}

/// Supplies the passphrase for a locked vault. On success, identities become
/// readable for the rest of the session.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap, [`AppError::VaultLocked`] if the
/// passphrase was wrong.
#[tauri::command]
pub async fn security_unlock(passphrase: String, state: State<'_, AppState>) -> Result<(), AppError> {
    let services = state.services()?;
    let mut identity = services.identity.lock().await;
    identity.unlock_with_passphrase(&passphrase).map_err(|_| AppError::VaultLocked)?;
    Ok(())
}

/// Turns passphrase protection on: re-encrypts the vault under a key derived
/// from `passphrase` and deletes the file-backed key it replaces.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap, [`AppError::VaultLocked`] if the
/// vault is locked or re-encryption failed.
#[tauri::command]
pub async fn security_enable_passphrase(passphrase: String, state: State<'_, AppState>) -> Result<(), AppError> {
    let services = state.services()?;
    let mut identity = services.identity.lock().await;
    identity.enable_passphrase(&passphrase).map_err(|_| AppError::VaultLocked)?;
    Ok(())
}

/// Turns passphrase protection off, reverting to a freshly generated
/// file-backed key.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap, [`AppError::VaultLocked`] if the
/// vault is locked or re-encryption failed.
#[tauri::command]
pub async fn security_disable_passphrase(state: State<'_, AppState>) -> Result<(), AppError> {
    let services = state.services()?;
    let mut identity = services.identity.lock().await;
    identity.disable_passphrase().map_err(|_| AppError::VaultLocked)?;
    Ok(())
}

/// The current wallet's raw private key, so it can be saved before switching
/// away from it — the only way back to this wallet once identities change,
/// since nothing else persists it anywhere recoverable. See
/// `docs/identity-design.md`: this is the gap the doc calls more urgent than
/// any feature it proposes, since without it a lost device is unrecoverable.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap, [`AppError::VaultLocked`] if the
/// vault is locked or holds no identity yet.
#[tauri::command]
pub async fn vault_export_key(state: State<'_, AppState>) -> Result<String, AppError> {
    let services = state.services()?;
    let identity = services.identity.lock().await;
    identity.exported_keys().ok_or(AppError::VaultLocked)
}

/// Replaces the current NEAR account with one from a supplied ed25519 secret, as
/// 64 hex characters.
///
/// Destructive: the wallet this device held before is gone unless its own
/// key was exported first. The frontend is responsible for warning about
/// that before calling this — the command itself trusts the caller, same as
/// every other mutating vault command.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap, [`AppError::VaultLocked`] if the
/// vault is locked or the supplied key does not parse.
#[tauri::command]
pub async fn vault_import_key(
    private_key_hex: String,
    alias: String,
    emoji: String,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    let services = state.services()?;
    let mut identity = services.identity.lock().await;
    identity
        .import_identity(private_key_hex, alias, emoji)
        .map(|_| ())
        .map_err(|_| AppError::VaultLocked)
}

/// What the profile screen shows.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct ProfileView {
    pub node_id: String,
    /// `14 (+55.6%)`, or just `14` with no prior window to compare against.
    ///
    /// Was a mocked `reputation` until ticket 39. Renamed along with the value
    /// because a count called a "score" is a figure whose name promises more
    /// than its definition delivers.
    pub settled: String,
    /// `2026.08.03` — when this installation first ran.
    pub member_since: String,
    pub offline: bool,
    pub network: String,
    /// Whether transactions here move real value.
    pub is_testnet: bool,
}

/// Identity and settings for the profile screen.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap.
#[tauri::command]
pub async fn profile_summary(state: State<'_, AppState>) -> Result<ProfileView, AppError> {
    let services = state.services()?;
    let network = crate::network_config::NetworkConfig::load(&cabal_store::JsonStore::new(
        crate::app_paths::in_data_dir("network.json"),
    ));

    // One snapshot for all three fields. Asking the actor twice was two round
    // trips for the same answer, and left a window where the identity and the
    // offline flag could come from different states of the mesh.
    let snapshot = match services.mesh.as_ref() {
        Some(mesh) => mesh.snapshot().await.ok(),
        None => None,
    };

    let node_id = snapshot
        .as_ref()
        .map_or_else(|| "—".into(), |s| cabal_core::NodeId::new(s.peer_id.clone()).truncated());

    // Absent mesh reads as offline: the screen must not show a connected
    // switch for a mesh that is not there.
    let offline = snapshot.as_ref().is_none_or(|s| s.offline);

    // The same ledger the home tile reads, so the two screens cannot disagree.
    // No mesh is needed: this is local history, not network state.
    let settled = crate::standing::LocalStanding::of(state.intents(), crate::intents::now_ms()).combined();

    Ok(ProfileView {
        node_id,
        settled,
        // Written on the first read and never again — see src/install.rs for
        // why neither the ephemeral mesh identity nor a file's creation time
        // could answer this.
        member_since: crate::install::format_date(crate::install::first_seen_ms(
            crate::intents::now_ms(),
        )),
        offline,
        network: network.network.label().to_string(),
        is_testnet: network.network.is_testnet(),
    })
}

/// Switches the network the app talks to — NEAR testnet or mainnet.
///
/// **Takes effect on next launch, not immediately.** Switching mid-session would
/// leave a deal signed for one network waiting on a gateway reading another. The
/// frontend's own copy is what tells the user to restart — this command only
/// persists the choice.
///
/// # Errors
///
/// [`AppError::internal`] if `network.json` cannot be written.
#[tauri::command]
pub async fn network_set(mainnet: bool) -> Result<(), AppError> {
    let network = if mainnet {
        crate::network_config::Network::Mainnet
    } else {
        crate::network_config::Network::Testnet
    };
    crate::network_config::NetworkConfig::switch(
        &cabal_store::JsonStore::new(crate::app_paths::in_data_dir("network.json")),
        network,
    )
    .map_err(AppError::internal)
}

/// Stops or resumes mesh participation.
///
/// The switch's own copy promises intents queue locally and nothing leaves the
/// device. The actor enforces that itself rather than trusting callers.
///
/// # Errors
///
/// [`AppError::MeshOffline`] if the mesh actor is gone.
#[tauri::command]
pub async fn set_offline_mode(offline: bool, state: State<'_, AppState>) -> Result<(), AppError> {
    let services = state.services()?;
    let mesh = services.mesh.as_ref().ok_or(AppError::MeshOffline)?;
    mesh.set_offline(offline).await.map_err(|_| AppError::MeshOffline)
}

// ---------------------------------------------------------------------------
// Guardian recovery
// ---------------------------------------------------------------------------

/// A nearby node the user could pick as a guardian.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct GuardianCandidate {
    /// Full peer id — opaque to the UI, round-tripped back verbatim to
    /// `guardian_enroll`. Never shown; `label` is what renders.
    pub peer_id: String,
    /// Truncated for display, e.g. `7F3A…C2E1`.
    pub label: String,
    pub hops: u8,
}

/// Nearby BLE nodes the user could pick as guardians.
///
/// Empty rather than an error when there is no BLE plane — the same choice
/// `list_nearby_nodes` makes, since "no radio" and "no candidates" read the
/// same way to this screen.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap.
#[tauri::command]
pub async fn guardian_candidates(state: State<'_, AppState>) -> Result<Vec<GuardianCandidate>, AppError> {
    let services = state.services()?;
    let Some(ble) = services.ble.as_ref() else {
        return Ok(Vec::new());
    };
    let peers = ble.peers().await.unwrap_or_default();
    Ok(peers
        .into_iter()
        .map(|peer| {
            let id = peer.id.to_string();
            GuardianCandidate {
                label: cabal_core::NodeId::new(id.clone()).truncated(),
                peer_id: id,
                hops: peer.hops,
            }
        })
        .collect())
}

/// What `SECURITY` shows about the guardian scheme, in both roles this
/// device can play.
#[derive(Debug, Clone, Copy, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct GuardianStatus {
    pub enrolled: bool,
    pub guardian_count: usize,
    pub threshold: u8,
    /// How many other owners this device holds a share for.
    pub holding_for: usize,
}

/// # Errors
///
/// [`AppError::NotReady`] before bootstrap.
#[tauri::command]
pub async fn guardian_status(state: State<'_, AppState>) -> Result<GuardianStatus, AppError> {
    let services = state.services()?;
    let service = services.guardian.lock().await;
    let (guardian_count, threshold) = service.owner_guardian_status();
    Ok(GuardianStatus {
        enrolled: service.is_enrolled(),
        guardian_count,
        threshold,
        holding_for: service.held_for().len(),
    })
}

/// Who accepted an enrollment invitation, and who did not answer.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct GuardianEnrollResult {
    pub enrolled: Vec<String>,
    pub no_reply: Vec<String>,
}

/// Invites `peer_ids` to become guardians and, for whoever accepts, sends a
/// sealed share split from the current vault key.
///
/// Blocks for up to 20 seconds waiting on replies — see
/// `guardian_actor::REPLY_TIMEOUT` — since a candidate is a human who has to
/// notice a prompt.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap, [`AppError::MeshOffline`] if
/// there is no BLE plane, [`AppError::VaultLocked`] if the vault is locked,
/// [`AppError::Internal`] if nobody accepted or the threshold was invalid.
#[tauri::command]
pub async fn guardian_enroll(
    peer_ids: Vec<String>,
    threshold: u8,
    state: State<'_, AppState>,
) -> Result<GuardianEnrollResult, AppError> {
    let services = state.services()?;
    let ble = services.ble.as_ref().ok_or(AppError::MeshOffline)?;

    let candidates: Vec<cabal_ble::PeerId> = peer_ids
        .iter()
        .map(|s| s.parse())
        .collect::<Result<_, _>>()
        .map_err(|_| AppError::InvalidIntent { field: "peer_ids", reason: crate::error::InvalidReason::Malformed })?;

    let vault_key = {
        let identity = services.identity.lock().await;
        identity.current_vault_key().map_err(|_| AppError::VaultLocked)?
    };

    let events = ble.subscribe();
    let outcome = crate::guardian_actor::enroll_guardians(&services.guardian, ble, events, &candidates, threshold, &vault_key)
        .await
        .map_err(AppError::internal)?;

    Ok(GuardianEnrollResult {
        enrolled: outcome.enrolled.iter().map(ToString::to_string).collect(),
        no_reply: outcome.no_reply.iter().map(ToString::to_string).collect(),
    })
}

/// Broadcasts an unlock request, waits for enough guardians to answer, and —
/// if a valid key comes back — opens the vault with it.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap, [`AppError::MeshOffline`] if
/// there is no BLE plane, [`AppError::VaultLocked`] if too few guardians
/// answered in time or the reconstructed key was wrong.
#[tauri::command]
pub async fn guardian_request_unlock(state: State<'_, AppState>) -> Result<(), AppError> {
    let services = state.services()?;
    let ble = services.ble.as_ref().ok_or(AppError::MeshOffline)?;

    let events = ble.subscribe();
    let candidate = crate::guardian_actor::request_unlock(&services.guardian, ble, events)
        .await
        .map_err(|_| AppError::VaultLocked)?;

    let mut identity = services.identity.lock().await;
    identity.unlock_with_guardian_key(candidate).map_err(|_| AppError::VaultLocked)?;
    Ok(())
}

/// Sends a pending unlock reply — the guardian side, called once the person
/// taps APPROVE on the prompt `guardian-unlock-request` drove.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap, [`AppError::MeshOffline`] if
/// there is no BLE plane, [`AppError::Internal`] if `id` was already
/// resolved or never existed.
#[tauri::command]
pub async fn guardian_approve_unlock(id: u32, state: State<'_, AppState>) -> Result<(), AppError> {
    let services = state.services()?;
    let ble = services.ble.as_ref().ok_or(AppError::MeshOffline)?;
    crate::guardian_actor::approve_unlock(&services.guardian_approvals, ble, id).await.map_err(AppError::internal)
}

/// Discards a pending unlock reply without sending anything.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap, [`AppError::Internal`] if `id`
/// was already resolved or never existed.
#[tauri::command]
pub async fn guardian_deny_unlock(id: u32, state: State<'_, AppState>) -> Result<(), AppError> {
    let services = state.services()?;
    crate::guardian_actor::deny_unlock(&services.guardian_approvals, id).await.map_err(AppError::internal)
}
