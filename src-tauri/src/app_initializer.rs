use crate::mesh::{MeshEvent, MeshNetwork};
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;

#[derive(Clone, serde::Serialize)]
struct BootstrapStatus {
    phase: String,
    message: String,
    progress: u8, // 0-100
}

pub struct SystemBootstrap;

impl SystemBootstrap {
    /// 1. Phase 1 (Sync): whatever the chain can tell us before the mesh starts.
    ///
    /// The balances themselves are read when the Vault asks, because the vault
    /// may still be locked here. All this keeps fresh is the verifier salt, so a
    /// device that loses its route still has one to sign with.
    pub async fn phase_1_sync(identity: &crate::identity::IdentityStore, http: &reqwest::Client, app: &AppHandle) {
        Self::emit(app, "PHASE_1_SYNC", "Checking connection...", 10);

        let Some(keypair) = identity.primary_keypair() else {
            Self::emit(app, "PHASE_1_SYNC", "Vault locked. Skipping chain sync.", 30);
            return;
        };
        Self::emit(app, "PHASE_1_SYNC", &format!("Identity: {}", keypair.account_id()), 20);

        let config = crate::network_config::NetworkConfig::current();
        let bridge = crate::near_bridge::NearBridge::new(config.rpc_url(), &config.verifier(), keypair, http.clone());
        match bridge {
            Ok(bridge) => match bridge.current_salt().await {
                Ok(salt) => {
                    crate::near_settlement::remember_salt(&crate::near_settlement::salt_store(), salt);
                    Self::emit(app, "PHASE_1_SYNC", "NEAR reachable.", 30);
                }
                Err(error) => {
                    tracing::info!(%error, "could not reach NEAR at startup");
                    Self::emit(app, "PHASE_1_SYNC", "Offline Mode. Using last known state.", 30);
                }
            },
            Err(error) => tracing::warn!(%error, "bad NEAR configuration"),
        }
    }

    /// 2. Phase 2 (Network): Init Libp2p
    /// Returns the initialized MeshNetwork and channels
    pub async fn phase_2_network(
        app: &AppHandle,
    ) -> Result<
        (
            MeshNetwork,
            crate::mesh_handle::MeshHandle,
            mpsc::UnboundedReceiver<MeshEvent>,
            tokio::sync::mpsc::Receiver<crate::mesh_handle::MeshCommand>,
            mpsc::UnboundedSender<MeshEvent>,
        ),
        String,
    > {
        Self::emit(app, "PHASE_3_NETWORK", "Booting Libp2p Swarm...", 70);
        
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        // Bounded: a caller that outruns the actor waits rather than
        // growing a queue until the process is killed.
        let (mesh_handle, command_rx) = crate::mesh_handle::MeshHandle::channel();

        match MeshNetwork::new().await {
            Ok(mesh) => {
                 Self::emit(app, "PHASE_3_NETWORK", &format!("PeerID Generated: {}", mesh.swarm.local_peer_id()), 85);
                 Ok((mesh, mesh_handle, event_rx, command_rx, event_tx))
            }
            Err(e) => {
                Self::emit(app, "PHASE_3_ERROR", &format!("Mesh Failed: {}", e), 0);
                Err(e.to_string())
            }
        }
    }

    // Helper: Emit UI Event
    fn emit(app: &AppHandle, phase: &str, msg: &str, progress: u8) {
        let _ = app.emit("bootstrap-status", BootstrapStatus {
            phase: phase.to_string(),
            message: msg.to_string(),
            progress,
        });
        tracing::info!(target: "cabalmesh::bootstrap", phase, progress, "{msg}");
    }
}
