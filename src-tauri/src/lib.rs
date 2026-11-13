mod app_initializer;
mod app_paths;
mod platform;
mod ollama_manager;
pub mod agent;
pub mod agent_tools;
pub mod price_feed;
mod negotiation;

// Public because their types *are* the IPC contract: everything below
// serializes across the boundary to the webview, so the shapes are already
// public API whether or not Rust says so. `tests/ipc_contract.rs` pins them.
pub mod balances;
pub mod identity;
pub mod wallet_setup;
pub mod mesh;

/// JSON-RPC to NEAR. See src/near_bridge.rs.
pub mod near_bridge;

/// Signed diffs over the mesh, settled through Intents. See src/near_settlement.rs.
pub mod near_settlement;

/// Request handle onto the mesh actor. See src/mesh_handle.rs.
pub mod mesh_handle;

/// The BLE plane: the offline core, when there is no network at all.
///
/// Separate from [`mesh`] on purpose. That module is the IP plane — gossipsub,
/// mDNS, the relay — and it needs a network to exist. This one needs only the
/// radio in the user's pocket. See docs/ble-mesh-design.md.
pub mod ble;

/// Bootstrap peer configuration. See src/bootstrap_config.rs.
pub mod bootstrap_config;

/// NEAR network selection and contract ids. See src/network_config.rs.
pub mod network_config;
mod llm_json;
mod lifecycle;
mod telemetry;
mod vault_key;
mod security_state;
mod unlock_attempts;
mod presence;
pub mod guardian;
pub mod guardian_actor;
pub mod intent_chat;
pub mod llm_budget;
pub mod llm_client;
pub mod llm_config;

/// The Android Wi-Fi multicast lock mDNS needs. See src/multicast.rs.
pub mod multicast;

/// Android's platform trust store, which rustls will not start without.
/// See src/tls.rs.
pub mod tls;

/// What this node has actually settled. See src/standing.rs.
pub mod standing;

/// Every intent this device has composed. See src/intents.rs.
pub mod intents;

/// When this installation first ran. See src/install.rs.
pub mod install;

/// Managed application state. See src/state.rs.
pub mod state;

/// Lifecycle for live frontend streams. See src/subscriptions.rs.
pub mod subscriptions;

/// The reshaped command surface. See src/commands.rs.
pub mod commands;

/// Presentation contracts shared with the frontend. See src/bindings.rs.
pub mod bindings;

/// The typed error union that crosses the IPC boundary. See src/error.rs.
pub mod error;

use app_initializer::SystemBootstrap;
use ollama_manager::OllamaManager;
use identity::IdentityStore;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use tokio::sync::Mutex;
use tauri::{Manager, Emitter};


#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// Surfaces a guardian notice: an event for the screen that is open, and a
/// system notification for when none is — a restore of this vault running
/// somewhere else is exactly the moment the person may not be looking.
fn guardian_notify<R: tauri::Runtime>(app: &tauri::AppHandle<R>, notice: guardian_actor::GuardianNotice) {
    use tauri_plugin_notification::NotificationExt;
    match notice {
        guardian_actor::GuardianNotice::Approval { id, kind, from, enrolled_at } => {
            let restore = kind == guardian_actor::ApprovalKind::Restore;
            let prompt = bindings::GuardianUnlockPrompt {
                id,
                from: cabal_core::NodeId::new(from.to_string()).truncated(),
                kind: if restore { bindings::GuardianPromptKind::Restore } else { bindings::GuardianPromptKind::Unlock },
                enrolled_at: enrolled_at.map(|at| at.to_rfc3339()),
            };
            let _ = app.emit("guardian-unlock-request", prompt);
            let body = if restore {
                "A new device asks to restore a vault you guard. Approve only if you can see its owner."
            } else {
                "A node asks you to help open their vault. Approve only if you can see them."
            };
            let _ = app.notification().builder().title("CabalMesh — guardian request").body(body).show();
        }
        guardian_actor::GuardianNotice::RestorePending { release_at } => {
            let _ = app.emit(
                "guardian-restore-pending",
                bindings::GuardianRestorePendingNotice { release_at: release_at.to_rfc3339() },
            );
            let _ = app
                .notification()
                .builder()
                .title("CabalMesh — restore in progress")
                .body("Someone is restoring your vault on another device. If this isn't you, open CabalMesh and cancel it.")
                .show();
        }
    }
}

pub fn run() {
    // First thing, before anything can emit: on a device this is the only
    // channel that reaches a developer, so nothing useful is logged until it
    // is installed.
    telemetry::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        // A guardian approval or a restore-in-progress warning has to reach
        // the person even when the app is not in front.
        .plugin(tauri_plugin_notification::init())
        // Registers the JNI handle only. Nothing here is reachable over IPC —
        // see src/multicast.rs for why the webview gets no grant for it.
        .plugin(multicast::init())
        // Registered before anything that can make an HTTPS request: on Android
        // rustls has no trust store until this runs. See src/tls.rs.
        .plugin(tls::init())
        // The Android BLE radio. Registers a handle on Android and a no-op
        // elsewhere, so the app builds identically on every platform.
        .plugin(tauri_plugin_cabal_ble::init())
        // The Android Keystore. Registered so the PIN path's presence secret
        // can be reached; a no-op elsewhere.
        .plugin(tauri_plugin_cabal_keystore::init())
        .setup(|app| {
            // The PIN path's presence secret on Android, if this device can
            // gate one on the screen lock (API 30+, lock set). See presence.rs.
            #[cfg(target_os = "android")]
            {
                use tauri_plugin_cabal_keystore::CabalKeystoreExt;
                let handle = app.handle().clone();
                if handle.cabal_keystore().presence_available() {
                    presence::install_android_source(Box::new(move || {
                        handle.cabal_keystore().presence_secret().map_err(|error| error.to_string())
                    }));
                }
            }
            // Synchronously, before the webview exists. Bootstrap fills the
            // services in afterwards; until then commands get NotReady rather
            // than a panic from an unmanaged type.
            // Before anything can persist: Tauri knows the correct directory
            // on every platform, and a mobile sandbox has no other right answer.
            match app.path().app_data_dir() {
                Ok(dir) => app_paths::set(dir),
                Err(error) => tracing::error!(
                    target: "cabalmesh::paths",
                    %error,
                    "platform gave no app data directory; falling back"
                ),
            }

            let state = state::AppState::new()?;
            app.manage(state.clone());

            let app_handle = app.handle().clone();

            // Only when this device's settings call for it. See `start_for`.
            let llm_settings = llm_config::load();
            let llm_budget = Arc::new(llm_budget::LlmBudget::new(llm_settings.calls_per_hour));
            let ollama_manager = Arc::new(OllamaManager::new());
            let ollama_init = ollama_manager.clone();
            tauri::async_runtime::spawn(async move {
                ollama_init.start_for(&llm_settings).await;
            });

            let ollama_state = ollama_manager.clone();

            // Initialize System via Bootstrap Workflow
            tauri::async_runtime::spawn(async move {
                // Desktop only: there is no .env file in a mobile bundle, and no
                // environment to read it into. `NetworkConfig` layers this on top
                // of network.json on desktop and falls through to the compiled-in
                // per-network table on mobile.
                #[cfg(desktop)]
                dotenv::dotenv().ok();

                let identity = Arc::new(Mutex::new(IdentityStore::new()));
                let http = reqwest::Client::new();
                let guardian_service = Arc::new(Mutex::new(guardian::GuardianService::open(&app_paths::data_dir())));
                // File mode: the vault is already open, so shares held for
                // others can be too. Passphrase mode attaches on unlock.
                let startup_key = {
                    let identity = identity.lock().await;
                    if identity.is_locked() { None } else { identity.current_vault_key().ok() }
                };
                if let Some(key) = startup_key {
                    if let Err(error) = guardian_service.lock().await.attach_vault_key(&key) {
                        tracing::warn!(target: "cabalmesh::guardian", %error, "held shares did not open");
                    }
                }
                let guardian_approvals = guardian_actor::PendingApprovals::new();

                // 1. Phase 1
                SystemBootstrap::phase_1_sync(&*identity.lock().await, &http, &app_handle).await;

                // Held pairs and unconfirmed reports wait on the chain, not on any
                // mesh event, so something has to keep asking. Does nothing while
                // there is nothing held.
                let agents = Arc::new(negotiation::Agents::default());
                let price_feed = Arc::new(price_feed::PriceFeed::open(
                    network_config::NetworkConfig::current().near().price_feed,
                    http.clone(),
                    cabal_store::DocumentStore::new(app_paths::in_data_dir("near_prices.json")),
                ));
                let tick_state = state.clone();
                tokio::spawn(async move {
                    loop {
                        tokio::time::sleep(negotiation::TICK_INTERVAL).await;
                        negotiation::tick(&tick_state);
                    }
                });

                let retry_state = state.clone();
                tokio::spawn(async move {
                    loop {
                        tokio::time::sleep(near_settlement::RETRY_INTERVAL).await;
                        near_settlement::retry(&retry_state).await;
                    }
                });

                // 2. Phase 2 & Network Start
                // The BLE plane starts independently of the swarm, because it
                // is independent: it is what the app does when there is no
                // network for the swarm to use. A failure to start either one
                // must leave the other running.
                let chosen = ble::backend::choose_for_app(&app_handle);
                let ble_transport = chosen
                    .as_ref()
                    .map_or_else(String::new, |t| t.describe().to_string());
                let ble = chosen.map(|transport| {
                    let (handle, mut events) = ble::spawn(
                        transport,
                        ble::fresh_identity(),
                        cabal_ble::peers::Capabilities::none(),
                    );
                    let forward = app_handle.clone();
                    tokio::spawn(async move {
                        loop {
                            match events.recv().await {
                                Ok(event) => {
                                    let _ = forward.emit("ble-event", format!("{event:?}"));
                                }
                                // A burst outpaced this consumer; the next
                                // recv picks up from there rather than
                                // stalling on events already gone.
                                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                            }
                        }
                    });

                    // Answers guardian traffic addressed to this device as a
                    // guardian for someone else, for as long as the app
                    // runs — a separate subscription from the forwarder
                    // above, since only one consumer can drain any one of
                    // them (see `BleHandle::subscribe`'s docs).
                    let mut guardian_events = handle.subscribe();
                    let guardian_ble = handle.clone();
                    let guardian_state = Arc::clone(&guardian_service);
                    let guardian_approvals_for_task = guardian_approvals.clone();
                    let guardian_app_handle = app_handle.clone();
                    tokio::spawn(async move {
                        loop {
                            match guardian_events.recv().await {
                                Ok(ble::BleEvent::Received { from, kind: cabal_ble::wire::PacketKind::Guardian, payload }) => {
                                    if let Ok(message) = cabal_guardian::protocol::GuardianMessage::from_bytes(&payload) {
                                        if let Some(notice) = guardian_actor::respond_to_guardian_traffic(
                                            &guardian_state,
                                            &guardian_ble,
                                            &guardian_approvals_for_task,
                                            from,
                                            message,
                                        )
                                        .await
                                        {
                                            guardian_notify(&guardian_app_handle, notice);
                                        }
                                    }
                                }
                                Ok(_) => {}
                                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                            }
                        }
                    });

                    // Guardian side: keep announcing restores this device is
                    // holding, so the original device hears about one even if
                    // it only comes into range partway through the delay.
                    let announce_ble = handle.clone();
                    let announce_state = Arc::clone(&guardian_service);
                    tokio::spawn(async move {
                        loop {
                            tokio::time::sleep(guardian::ANNOUNCE_INTERVAL).await;
                            guardian_actor::announce_pending_restores(&announce_state, &announce_ble).await;
                        }
                    });

                    // Owner side: keep guardians' backup copies and value flag
                    // current. Does nothing until a restore-capable enrollment
                    // exists, or while the vault is locked.
                    let backup_ble = handle.clone();
                    let backup_guardian = Arc::clone(&guardian_service);
                    let backup_identity = Arc::clone(&identity);
                    let backup_state = state.clone();
                    tokio::spawn(async move {
                        loop {
                            tokio::time::sleep(guardian::BACKUP_REFRESH_INTERVAL).await;
                            let plaintext = { backup_identity.lock().await.backup_plaintext().ok() };
                            let Some(plaintext) = plaintext else { continue };
                            let has_value = commands::vault_has_value(&backup_state).await;
                            if let Err(error) =
                                guardian_actor::broadcast_backup_update(&backup_guardian, &backup_ble, &plaintext, has_value).await
                            {
                                tracing::warn!(target: "cabalmesh::guardian", %error, "backup refresh failed");
                            }
                        }
                    });

                    // Bridges an offline peer's BLE-broadcast intent onto the
                    // IP mesh, once this device has one to bridge onto. This
                    // is the other half of `commands.rs`'s `publish_over_ble`
                    // fallback: an intent composed with no Wi-Fi floods over
                    // Bluetooth instead of just queueing, and whichever
                    // nearby device is currently online — the gateway from
                    // the connectivity fix above — is the one that actually
                    // republishes it where the rest of the mesh can see it.
                    // Gated on `runtime_caps().online` rather than merely
                    // `services.mesh` existing: a mesh handle can exist with
                    // zero connected peers, and `MeshHandle::publish` treats
                    // that as a harmless local no-op rather than an error, so
                    // checking only "is there a handle" would silently drop
                    // the very peers this bridge exists to reach.
                    let mut intent_bridge_events = handle.subscribe();
                    let intent_bridge_state = state.clone();
                    let intent_bridge_app = app_handle.clone();
                    tokio::spawn(async move {
                        loop {
                            match intent_bridge_events.recv().await {
                                Ok(ble::BleEvent::Received { kind: cabal_ble::wire::PacketKind::Intent, payload, .. }) => {
                                    let Ok(intent) = serde_json::from_slice::<mesh::PrivacyIntent>(&payload) else {
                                        continue;
                                    };
                                    // A deal agreed over the radio has to reach
                                    // the ledger exactly as one agreed over IP.
                                    // The flood resends, and every handler here
                                    // is idempotent, so no dedup is needed.
                                    let settlement = mesh::classify_settlement(&intent);
                                    if let Some(event) = &settlement {
                                        intents::apply_mesh_event(intent_bridge_state.received(), event);
                                        near_settlement::dispatch(&intent_bridge_state, event);
                                        negotiation::dispatch(&intent_bridge_state, event);
                                        let _ = intent_bridge_app.emit("mesh-event", event);
                                    }
                                    // Counted on arrival, ahead of the
                                    // online-gate below: a "received" count
                                    // that only incremented when this device
                                    // happened to have a gateway to bridge
                                    // onto would undercount the very radio
                                    // traffic it exists to show. The same
                                    // "was this new" result also gates
                                    // negotiation, so a BLE resend of the
                                    // same intent can't propose the same deal
                                    // twice — see `ReceivedLog::record`.
                                    if settlement.is_none() && intent_bridge_state.received().record(&intent.id) {
                                        let negotiation_state = intent_bridge_state.clone();
                                        let candidate = intent.clone();
                                        tokio::spawn(async move {
                                            negotiation::consider(&negotiation_state, &candidate).await;
                                        });
                                    }

                                    if !intent_bridge_state.runtime_caps().online {
                                        continue;
                                    }
                                    let Ok(services) = intent_bridge_state.services() else { continue };
                                    let Some(mesh_handle) = services.mesh.as_ref() else { continue };
                                    match mesh_handle.publish(intent).await {
                                        Ok(()) => tracing::info!(
                                            "bridged a BLE intent from an offline peer onto the IP mesh"
                                        ),
                                        Err(error) => tracing::debug!(
                                            %error,
                                            "could not bridge a BLE intent onto the IP mesh"
                                        ),
                                    }
                                }
                                Ok(_) => {}
                                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                            }
                        }
                    });

                    handle
                });
                if ble.is_none() {
                    tracing::info!(
                        "no BLE transport for this build; the offline plane is not running \
                         (set {} to link two machines over TCP)",
                        ble::backend::LOOPBACK_ENV
                    );
                }

                match SystemBootstrap::phase_2_network(&app_handle).await {
                    Ok((mut mesh, mesh_handle, mut event_rx, command_rx, event_tx)) => {
                        tracing::info!("✅ System Bootstrap Complete. Mesh Swarm Active.");

                        let relay_bytes = mesh.relay_bytes.clone();

                        // Start Mesh Loop (Background)
                        tokio::spawn(async move {
                            if let Err(e) = mesh.start(event_tx, command_rx).await {
                                tracing::warn!("Mesh network error: {}", e);
                            }
                        });

                        // Forward Mesh Events to Frontend, applying them to the
                        // intent ledger on the way through.
                        //
                        // Applied here rather than polled from a command: the
                        // detail screen has to reflect negotiation as it
                        // happens, and a five-second poll would show a bid
                        // arriving five seconds after the peer sent it.
                        let handle_clone = app_handle.clone();
                        let received = state.received().clone();
                        let state_for_events = state.clone();
                        let ble_for_events = ble.clone();
                        tokio::spawn(async move {
                            while let Some(event) = event_rx.recv().await {
                                // Gated on "was this id new" so a duplicate
                                // delivery of the same intent (gossipsub can
                                // redeliver) can't trigger a second match —
                                // same reasoning as the BLE-bridge path below.
                                if let mesh::MeshEvent::IntentReceived { intent } = &event {
                                    if received.record(&intent.id) {
                                        let negotiation_state = state_for_events.clone();
                                        let candidate = intent.clone();
                                        tokio::spawn(async move {
                                            negotiation::consider(&negotiation_state, &candidate).await;
                                        });
                                    }
                                }
                                intents::apply_mesh_event(&received, &event);
                                near_settlement::dispatch(&state_for_events, &event);
                                negotiation::dispatch(&state_for_events, &event);
                                // A device with no IP-plane connectivity has
                                // nothing to offer a BLE peer asking for a
                                // gateway; one that just regained it does.
                                // This is the only place either fact is
                                // learned, so it is also the only place that
                                // can keep `RuntimeCaps.online` and the BLE
                                // engine's gateway bit from going stale.
                                if let mesh::MeshEvent::ConnectivityChanged { online } = &event {
                                    let mut caps = state_for_events.runtime_caps();
                                    caps.online = *online;
                                    state_for_events.set_runtime_caps(caps);

                                    if let Some(ble) = &ble_for_events {
                                        if let Err(error) = ble.set_gateway(*online).await {
                                            tracing::warn!(%error, "failed to update BLE gateway capability");
                                        }
                                    }

                                    if *online {
                                        // Anything composed while offline is
                                        // sitting in `Draft` waiting for
                                        // exactly this reconnection.
                                        commands::retry_queued_intents(&state_for_events).await;
                                        near_settlement::retry(&state_for_events).await;
                                    }
                                }
                                let _ = handle_clone.emit("mesh-event", event);
                            }
                        });

                        state.set_services(state::Services {
                            mesh: Some(mesh_handle),
                            ble: ble.clone(),
                            ble_transport: ble_transport.clone(),
                            ollama: ollama_state,
                            llm_budget,
                            agents,
                            price_feed,
                            identity,
                            http,
                            guardian: guardian_service.clone(),
                            guardian_approvals: guardian_approvals.clone(),
                            relay_bytes,
                        });

                        // Only once the mesh is actually participating. The
                        // lock keeps the Wi-Fi radio in a higher-power state,
                        // so taking it before there is anything to discover
                        // would be a battery cost with no benefit.
                        multicast::refresh(&app_handle);
                    }
                    Err(e) => {
                        tracing::error!("❌ Bootstrap Failed: {}", e);
                        // Publish anyway: without a mesh the chain and vault
                        // commands still work, and the UI can say so. Leaving
                        // services unset would make every command NotReady
                        // forever, which reads as a hang rather than an error.
                        state.set_services(state::Services {
                            mesh: None,
                            ble,
                            ble_transport,
                            ollama: ollama_state,
                            llm_budget,
                            agents,
                            price_feed,
                            identity,
                            http,
                            guardian: guardian_service,
                            guardian_approvals,
                            relay_bytes: Arc::new(AtomicU64::new(0)),
                        });
                    }
                }
            });

            Ok(())
        })
        // One handler, every platform. Desktop used to carry a second,
        // frozen arm for the old RPG UI (see git history for `src/legacy/`);
        // now that UI is gone, desktop and mobile share the same reshaped
        // command surface — new commands join this single list as their
        // screens land, never speculatively.
        .invoke_handler(tauri::generate_handler![
            commands::unsubscribe,
            commands::session_status,
            commands::enter_mesh,
            commands::mesh_snapshot,
            commands::subscribe_mesh_log,
            commands::list_nearby_nodes,
            commands::ble_status,
            commands::list_intents,
            commands::intent_form_options,
            commands::llm_get_settings,
            commands::llm_save_settings,
            commands::llm_set_api_key,
            commands::llm_test_connection,
            commands::parse_intent_chat,
            commands::preview_intent,
            commands::broadcast_intent,
            commands::intent_detail,
            commands::subscribe_settlement_log,
            commands::settle_intent,
            commands::cancel_intent,
            commands::intent_proof,
            commands::vault_assets,
            commands::vault_refresh_balance,
            commands::vault_address,
            commands::near_account,
            wallet_setup::wallet_setup_status,
            wallet_setup::wallet_check_name,
            wallet_setup::wallet_create_testnet_account,
            commands::near_deposit,
            commands::near_withdraw,
            commands::near_unwrap,
            commands::rewards_status,
            commands::rewards_claim,
            commands::vault_identities,
            commands::vault_keys,
            commands::security_status,
            commands::security_unlock,
            commands::security_enable_passphrase,
            commands::security_disable_passphrase,
            commands::security_enable_pin,
            commands::vault_export_key,
            commands::vault_import_key,
            commands::guardian_candidates,
            commands::guardian_status,
            commands::guardian_enroll,
            commands::guardian_request_unlock,
            commands::guardian_approve_unlock,
            commands::guardian_deny_unlock,
            commands::guardian_cancel_restore,
            commands::guardian_restore_abandon,
            commands::guardian_restore_code,
            commands::guardian_restore_poll,
            commands::guardian_restore_start,
            commands::profile_summary,
            commands::network_set,
            commands::set_offline_mode,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // Mobile lifecycle. Tauri 2.11 propagates these from the platform;
            // 2.9 did not, which is why an earlier plan specified a custom
            // plugin that is no longer needed.
            #[cfg(mobile)]
            if let tauri::RunEvent::WindowEvent { event, .. } = &event {
                match event {
                    tauri::WindowEvent::Suspended => lifecycle::on_suspend(app),
                    tauri::WindowEvent::Resumed => lifecycle::on_resume(app),
                    _ => {}
                }
            }
            let _ = (app, event);
        });
}
