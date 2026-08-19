use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use tracing::debug;

use crate::agent_persona::AgentPersona;
use crate::secrets_vault::SecretsVault;
use crate::user_profile::UserProfile;

/// Unified profile, persona, and secrets management engine.
#[derive(Debug, Clone)]
pub struct ProfileManager {
    config_dir: PathBuf,
    user_profile: Arc<RwLock<UserProfile>>,
    agent_persona: Arc<RwLock<AgentPersona>>,
    secrets_vault: SecretsVault,
}

impl Default for ProfileManager {
    fn default() -> Self {
        Self::discover()
    }
}

impl ProfileManager {
    /// Discover the best configuration directory.
    pub fn discover() -> Self {
        if let Ok(dir) = std::env::var("COMPANION_CONFIG_DIR") {
            let path = PathBuf::from(dir);
            return Self::from_dir(path);
        }

        // Check local ./config directory
        let local_config = PathBuf::from("config");
        if local_config.exists() && (local_config.join("user.md").exists() || local_config.join("agent.md").exists()) {
            return Self::from_dir(local_config);
        }

        // Check ~/.companion directory
        if let Ok(home) = std::env::var("HOME") {
            let user_dir = PathBuf::from(home).join(".companion");
            if user_dir.exists() {
                return Self::from_dir(user_dir);
            }
        }

        // Fallback to local config dir
        Self::from_dir(local_config)
    }

    /// Load or initialize profile manager from a specific directory.
    pub fn from_dir<P: AsRef<Path>>(dir: P) -> Self {
        let config_dir = dir.as_ref().to_path_buf();
        let manager = Self {
            config_dir: config_dir.clone(),
            user_profile: Arc::new(RwLock::new(UserProfile::default())),
            agent_persona: Arc::new(RwLock::new(AgentPersona::default())),
            secrets_vault: SecretsVault::new(),
        };

        manager.ensure_templates();
        let _ = manager.reload();
        manager
    }

    /// Ensure default templates exist if missing.
    pub fn ensure_templates(&self) {
        if !self.config_dir.exists() {
            let _ = std::fs::create_dir_all(&self.config_dir);
        }

        let user_file = self.config_dir.join("user.md");
        if !user_file.exists() {
            let default_user = r#"# User Profile

## Identity
- **Name**: Rajat Malik
- **Handle**: @rajat
- **Timezone**: Asia/Kolkata (IST, UTC+5:30)

## Preferences
- Concise, technically precise communication
- Prefers Rust, TypeScript, Python
- Dark mode aesthetic

## Current Projects
- Companion: Enterprise Autonomous AI Agent Runtime (Rust)
"#;
            let _ = std::fs::write(&user_file, default_user);
        }

        let agent_file = self.config_dir.join("agent.md");
        if !agent_file.exists() {
            let default_agent = r#"# Agent Persona

## Identity
- **Name**: Companion
- **Role**: Autonomous AI Pair-Programmer & Enterprise Execution Runtime

## Personality
- Professional, sharp, and encouraging — never robotic or sluggish.
- Highly analytical, deterministic, and security-first.
- Proactively assesses risks before executing destructive actions.

## Behavioral Rules
- Address the user respectfully and directly.
- Always provide production-ready implementations.
- Validate state transitions against strict contracts.

## Tone
- Senior software architect & peer-level engineering companion.
"#;
            let _ = std::fs::write(&agent_file, default_agent);
        }

        let secrets_file = self.config_dir.join("secrets.toml");
        if !secrets_file.exists() {
            let default_secrets = r#"# Companion Secrets Vault (Local Instance)
# WARNING: Do NOT commit this file to Git.

[api_keys]
gemini_api_key = ""
openai_api_key = ""
github_token = ""
nvidia_api_key = ""

[database]
postgres_url = "postgresql://companion:companion_dev@localhost:5432/companion"

[custom]
"#;
            let _ = std::fs::write(&secrets_file, default_secrets);
        }
    }

    /// Reload user profile, agent persona, and secrets from disk.
    pub fn reload(&self) -> Result<(), std::io::Error> {
        let user_file = self.config_dir.join("user.md");
        if user_file.exists() {
            if let Ok(content) = std::fs::read_to_string(&user_file) {
                let parsed = UserProfile::from_markdown(&content);
                if let Ok(mut lock) = self.user_profile.write() {
                    *lock = parsed;
                }
                debug!("Loaded user profile from {}", user_file.display());
            }
        }

        let agent_file = self.config_dir.join("agent.md");
        if agent_file.exists() {
            if let Ok(content) = std::fs::read_to_string(&agent_file) {
                let parsed = AgentPersona::from_markdown(&content);
                if let Ok(mut lock) = self.agent_persona.write() {
                    *lock = parsed;
                }
                debug!("Loaded agent persona from {}", agent_file.display());
            }
        }

        let secrets_file = self.config_dir.join("secrets.toml");
        if secrets_file.exists() {
            if let Ok(content) = std::fs::read_to_string(&secrets_file) {
                let vault = SecretsVault::from_toml(&content);
                // Transfer keys
                for handle in vault.list_handles() {
                    if let Some(val) = vault.get(&handle) {
                        self.secrets_vault.set(&handle, &val);
                    }
                }
                debug!("Loaded secrets vault from {}", secrets_file.display());
            }
        }

        Ok(())
    }

    /// Retrieve the loaded UserProfile.
    pub fn user_profile(&self) -> UserProfile {
        self.user_profile.read().map(|p| p.clone()).unwrap_or_default()
    }

    /// Retrieve the loaded AgentPersona.
    pub fn agent_persona(&self) -> AgentPersona {
        self.agent_persona.read().map(|p| p.clone()).unwrap_or_default()
    }

    /// Access the SecretsVault.
    pub fn secrets(&self) -> &SecretsVault {
        &self.secrets_vault
    }

    /// Save secrets vault changes back to secrets.toml.
    pub fn save_secrets(&self) -> Result<(), std::io::Error> {
        let secrets_file = self.config_dir.join("secrets.toml");
        let toml_str = self.secrets_vault.to_toml_string();
        std::fs::write(&secrets_file, toml_str)
    }

    /// Update user.md content and reload.
    pub fn update_user_profile(&self, markdown: &str) -> Result<(), std::io::Error> {
        let user_file = self.config_dir.join("user.md");
        std::fs::write(&user_file, markdown)?;
        self.reload()
    }

    /// Update agent.md content and reload.
    pub fn update_agent_persona(&self, markdown: &str) -> Result<(), std::io::Error> {
        let agent_file = self.config_dir.join("agent.md");
        std::fs::write(&agent_file, markdown)?;
        self.reload()
    }

    /// Get path to the active configuration directory.
    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }
}
