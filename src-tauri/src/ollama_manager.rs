use std::process::{Command, Child, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::llm_config::{LlmSettings, Preset};
use crate::platform::CAN_SPAWN_PROCESSES;

fn no_local_ollama() -> String {
    "This platform cannot run a local Ollama server. Set a reachable base URL in Profile.".to_string()
}

/// Whether a base URL points at this machine, where this app could run the server.
fn is_this_machine(base_url: &str) -> bool {
    reqwest::Url::parse(base_url)
        .is_ok_and(|url| matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")))
}

/// Ollama's own API sits beside its OpenAI-compatible `/v1`.
fn tags_url(base_url: &str) -> String {
    format!("{}/api/tags", base_url.strip_suffix("/v1").unwrap_or(base_url))
}

pub struct OllamaManager {
    process: Arc<Mutex<Option<Child>>>,
}

impl OllamaManager {
    pub fn new() -> Self {
        OllamaManager { process: Arc::new(Mutex::new(None)) }
    }

    /// Brings Ollama up for these settings when this device should run it: the
    /// `ollama-local` preset, pointed at this machine, on a platform that can spawn
    /// a process. Otherwise it only reports whether the server answers.
    pub async fn start_for(&self, settings: &LlmSettings) {
        if settings.preset != Preset::OllamaLocal {
            return;
        }
        let url = &settings.base_url;
        let running = self.health_check(url).await;

        if !CAN_SPAWN_PROCESSES || !is_this_machine(url) {
            if running {
                tracing::info!("✅ Ollama at {} is healthy", url);
            } else {
                tracing::warn!("⚠️  No Ollama at {}. Set a reachable base URL in Profile.", url);
            }
            return;
        }

        if !self.is_installed() {
            tracing::warn!("⚠️  Ollama not found. Install it from https://ollama.ai");
            return;
        }
        if !running {
            if let Err(error) = self.start_service() {
                tracing::error!("❌ Failed to start Ollama: {}", error);
                return;
            }
            for attempt in 1..=10 {
                if self.health_check(url).await {
                    tracing::info!("✅ Ollama service is healthy");
                    break;
                }
                if attempt == 10 {
                    tracing::warn!("⚠️  Ollama service not responding");
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }

        let model = settings.model.trim().to_string();
        if !model.is_empty() {
            tokio::task::spawn_blocking(move || {
                if let Err(error) = pull_model(&model) {
                    tracing::warn!("⚠️  Warning: {}", error);
                }
            });
        }
    }

    /// Check if Ollama is installed
    pub fn is_installed(&self) -> bool {
        if !CAN_SPAWN_PROCESSES {
            return false;
        }

        Command::new("ollama")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok()
    }

    /// Start Ollama service in the background
    pub fn start_service(&self) -> Result<(), String> {
        if !CAN_SPAWN_PROCESSES {
            return Err(no_local_ollama());
        }

        if !self.is_installed() {
            return Err("Ollama is not installed. Please install it from https://ollama.ai".to_string());
        }

        tracing::info!("🤖 Starting Ollama service...");

        // Start ollama serve as a background process
        match Command::new("ollama")
            .arg("serve")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => {
                *self.process.lock().unwrap() = Some(child);
                tracing::info!("✅ Ollama service started");
                
                // Give it time to start
                thread::sleep(Duration::from_secs(2));
                
                Ok(())
            }
            Err(e) => Err(format!("Failed to start Ollama: {}", e)),
        }
    }

    /// Check if Ollama service is responding
    pub async fn health_check(&self, base_url: &str) -> bool {
        let client = reqwest::Client::new();
        match client.get(tags_url(base_url)).timeout(Duration::from_secs(2)).send().await {
            Ok(response) => response.status().is_success(),
            Err(_) => false,
        }
    }

    /// Stop the Ollama service
    pub fn stop_service(&self) {
        if let Some(mut child) = self.process.lock().unwrap().take() {
            tracing::info!("🛑 Stopping Ollama service...");
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for OllamaManager {
    fn drop(&mut self) {
        self.stop_service();
    }
}

/// Pull the AI model if not already available
fn pull_model(model: &str) -> Result<(), String> {
    if !CAN_SPAWN_PROCESSES {
        return Err(no_local_ollama());
    }

    tracing::info!("📥 Checking for model: {}", model);

    let list_output =
        Command::new("ollama").arg("list").output().map_err(|e| format!("Failed to list models: {}", e))?;
    if String::from_utf8_lossy(&list_output.stdout).contains(model) {
        tracing::info!("✅ Model {} already available", model);
        return Ok(());
    }

    tracing::info!("📥 Pulling model {} (this may take a few minutes)...", model);
    let output =
        Command::new("ollama").arg("pull").arg(model).output().map_err(|e| format!("Failed to pull model: {}", e))?;
    if output.status.success() {
        tracing::info!("✅ Model {} downloaded successfully", model);
        Ok(())
    } else {
        Err(format!("Failed to pull model: {}", String::from_utf8_lossy(&output.stderr)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_server_on_this_machine_is_ours_to_start() {
        assert!(is_this_machine("http://localhost:11434/v1"));
        assert!(is_this_machine("http://127.0.0.1:11434/v1"));
        assert!(!is_this_machine("http://192.168.1.9:11434/v1"));
        assert!(!is_this_machine("https://cloud-api.near.ai/v1"));
    }

    #[test]
    fn its_health_check_uses_ollamas_own_api() {
        assert_eq!(tags_url("http://localhost:11434/v1"), "http://localhost:11434/api/tags");
    }

    #[tokio::test]
    async fn a_hosted_preset_starts_nothing() {
        // Would try `ollama` if it got past the preset check; finishing is the point.
        let settings = LlmSettings {
            preset: Preset::OpenRouter,
            base_url: Preset::OpenRouter.base_url().into(),
            ..LlmSettings::default()
        };
        let manager = OllamaManager::new();
        manager.start_for(&settings).await;
        assert!(manager.process.lock().unwrap().is_none());
    }
}
