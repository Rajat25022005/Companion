use std::sync::Arc;
use companion_profile::{AgentPersona, ProfileManager, SecretsVault, UserProfile};

#[test]
fn test_user_profile_parsing() {
    let md = r#"# User Profile

## Identity
- **Name**: Alice Wonder
- **Handle**: @alice
- **Timezone**: America/New_York

## Preferences
- Prefers Rust and async Tokio
- Likes clean concise responses

## Current Projects
- Distributed Raft Engine in Rust

## Notes
- Working on low-latency networks.
"#;

    let profile = UserProfile::from_markdown(md);
    assert_eq!(profile.name.as_deref(), Some("Alice Wonder"));
    assert_eq!(profile.handle.as_deref(), Some("@alice"));
    assert_eq!(profile.timezone.as_deref(), Some("America/New_York"));
    assert_eq!(profile.preferences.len(), 2);
    assert_eq!(profile.current_projects.len(), 1);
    assert!(profile.notes.as_deref().unwrap().contains("low-latency"));

    let block = profile.as_context_block();
    assert!(block.contains("Alice Wonder"));
    assert!(block.contains("Distributed Raft Engine"));
}

#[test]
fn test_agent_persona_parsing() {
    let md = r#"# Agent Persona

## Identity
- **Name**: Sentinel
- **Role**: High-Assurance AI Verification Agent

## Personality
- Deterministic and rigorous
- Security-focused

## Behavioral Rules
- Verify every transition
- Never execute without contract

## Tone
- Objective and clear
"#;

    let persona = AgentPersona::from_markdown(md);
    assert_eq!(persona.name(), "Sentinel");
    assert_eq!(persona.role.as_deref(), Some("High-Assurance AI Verification Agent"));
    assert_eq!(persona.traits.len(), 2);
    assert_eq!(persona.behavioral_rules.len(), 2);
    assert_eq!(persona.tone.len(), 1);

    let prefix = persona.as_system_prompt_prefix();
    assert!(prefix.contains("Sentinel"));
    assert!(prefix.contains("High-Assurance AI Verification Agent"));
    assert!(prefix.contains("Verify every transition"));
}

#[test]
fn test_secrets_vault_isolation_and_handles() {
    let toml = r#"
[api_keys]
gemini_api_key = "gemini-super-secret-key-12345"
openai_api_key = "sk-openai-confidential-token-abc"

[database]
postgres_url = "postgres://admin:topsecret@localhost:5432/db"

[custom]
internal_service_key = "srv-xyz-987"
"#;

    let vault = SecretsVault::from_toml(toml);

    // Direct key resolution
    assert_eq!(vault.get("gemini_api_key"), Some("gemini-super-secret-key-12345".into()));
    assert_eq!(vault.get("internal_service_key"), Some("srv-xyz-987".into()));

    // Opaque handle resolution ($SECRET:...)
    assert_eq!(vault.resolve_handle("$SECRET:gemini_api_key"), Some("gemini-super-secret-key-12345".into()));
    assert_eq!(vault.resolve_handle("$SECRET:openai_api_key"), Some("sk-openai-confidential-token-abc".into()));
    assert_eq!(vault.resolve_handle("SECRET:database.postgres_url"), Some("postgres://admin:topsecret@localhost:5432/db".into()));

    // Handle listing returns keys only, never values
    let handles = vault.list_handles();
    assert!(handles.contains(&"gemini_api_key".to_string()));
    assert!(handles.contains(&"openai_api_key".to_string()));
    assert!(handles.contains(&"internal_service_key".to_string()));

    // Known values for redaction
    let known_values = vault.known_secret_values();
    assert!(known_values.contains(&"gemini-super-secret-key-12345".to_string()));
    assert!(known_values.contains(&"srv-xyz-987".to_string()));
}

#[test]
fn test_secrets_redaction_integration() {
    let toml = r#"
[api_keys]
gemini_api_key = "super-secret-gemini-key-999"
"#;
    let vault = SecretsVault::from_toml(toml);
    let redactor = companion_policy::SecurityRedactor::new().with_secrets(vault.known_secret_values());

    let raw_text = "Here is my key: super-secret-gemini-key-999 and another word";
    let sanitized = redactor.redact(raw_text);

    assert_eq!(sanitized, "Here is my key: [REDACTED_VAULT_SECRET] and another word");
    assert!(!sanitized.contains("super-secret-gemini-key-999"));
}

#[test]
fn test_profile_manager_file_lifecycle() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().to_path_buf();

    // Create manager on clean directory -> templates auto-created
    let manager = ProfileManager::from_dir(&dir);

    assert!(dir.join("user.md").exists());
    assert!(dir.join("agent.md").exists());
    assert!(dir.join("secrets.toml").exists());

    // Modify user profile
    let custom_user = r#"# User Profile

## Identity
- **Name**: Bob Builder
"#;
    manager.update_user_profile(custom_user).unwrap();
    assert_eq!(manager.user_profile().display_name(), "Bob Builder");

    // Modify secrets
    manager.secrets().set("custom_auth_token", "bearer-token-val-987");
    manager.save_secrets().unwrap();

    let reloaded_manager = ProfileManager::from_dir(&dir);
    assert_eq!(
        reloaded_manager.secrets().resolve_handle("$SECRET:custom_auth_token"),
        Some("bearer-token-val-987".into())
    );
}

#[tokio::test]
async fn test_context_compiler_with_persona_and_user_profile() {
    use companion_context::ContextCompiler;
    use companion_domain::{ContextBudget, ContextSources};

    let compiler = ContextCompiler::new();

    let sources = ContextSources {
        identity_policy: Some("Base system policy.".into()),
        agent_persona_block: Some("### Agent Persona: You are Guardian AI.".into()),
        user_profile_block: Some("### User Profile: User is Alice.".into()),
        ..Default::default()
    };

    let budget = ContextBudget::for_total_tokens(2048);
    let compiled = compiler.compile(&sources, &budget, None).await.unwrap();

    assert!(compiled.sections_included.contains(&"agent_persona".to_string()));
    assert!(compiled.sections_included.contains(&"identity_policy".to_string()));
    assert!(compiled.sections_included.contains(&"user_profile".to_string()));

    let sys_msg = &compiled.messages[0].content;
    assert!(sys_msg.contains("Guardian AI"));
    assert!(sys_msg.contains("Base system policy"));
    assert!(sys_msg.contains("Alice"));
}
