use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use toml::Value;

/// Secure in-memory secrets vault.
///
/// NOTE: `Serialize` is deliberately NOT implemented for `SecretsVault`
/// to guarantee credentials cannot be leaked into JSON responses, logs, or agent contexts.
#[derive(Debug, Clone)]
pub struct SecretsVault {
    secrets: Arc<RwLock<HashMap<String, String>>>,
}

impl Default for SecretsVault {
    fn default() -> Self {
        Self {
            secrets: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl SecretsVault {
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse secrets from TOML configuration content.
    pub fn from_toml(content: &str) -> Self {
        let vault = Self::new();
        if let Ok(Value::Table(table)) = content.parse::<Value>() {
            let mut map = HashMap::new();

            for (section, val) in table {
                match val {
                    Value::Table(sub_table) => {
                        for (k, v) in sub_table {
                            if let Value::String(s) = v {
                                let key_name = k.to_string();
                                let namespaced = format!("{section}.{k}");
                                map.insert(key_name, s.clone());
                                map.insert(namespaced, s);
                            }
                        }
                    }
                    Value::String(s) => {
                        map.insert(section, s);
                    }
                    _ => {}
                }
            }

            if let Ok(mut lock) = vault.secrets.write() {
                *lock = map;
            }
        }
        vault
    }

    /// Retrieve a secret value by key or namespaced key (e.g. "gemini_api_key" or "api_keys.gemini_api_key").
    pub fn get(&self, key: &str) -> Option<String> {
        let lock = self.secrets.read().ok()?;
        lock.get(key)
            .cloned()
            .filter(|v| !v.trim().is_empty())
    }

    /// Resolve an opaque handle (e.g. "$SECRET:gemini_api_key" or "gemini_api_key") into its secret value.
    pub fn resolve_handle(&self, handle: &str) -> Option<String> {
        let key = if let Some(stripped) = handle.strip_prefix("$SECRET:") {
            stripped.trim()
        } else if let Some(stripped) = handle.strip_prefix("SECRET:") {
            stripped.trim()
        } else {
            handle.trim()
        };

        self.get(key)
    }

    /// Check if a secret key exists and has a non-empty value.
    pub fn has(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// List all configured secret handles/keys (keys only, never values).
    pub fn list_handles(&self) -> Vec<String> {
        if let Ok(lock) = self.secrets.read() {
            let mut keys: Vec<String> = lock
                .iter()
                .filter(|(_, v)| !v.trim().is_empty())
                .map(|(k, _)| k.clone())
                .collect();
            keys.sort();
            keys
        } else {
            Vec::new()
        }
    }

    /// Set or update a secret value in the vault.
    pub fn set(&self, key: &str, value: &str) {
        if let Ok(mut lock) = self.secrets.write() {
            lock.insert(key.to_string(), value.to_string());
        }
    }

    /// Get all non-empty secret values for use in `SecurityRedactor` to catch accidental leakage.
    pub fn known_secret_values(&self) -> Vec<String> {
        if let Ok(lock) = self.secrets.read() {
            let mut values = Vec::new();
            for v in lock.values() {
                let trimmed = v.trim();
                // Only track non-trivial secrets (> 4 chars) to avoid false-positive redaction of short strings
                if trimmed.len() > 4 && !values.contains(&trimmed.to_string()) {
                    values.push(trimmed.to_string());
                }
            }
            values
        } else {
            Vec::new()
        }
    }

    /// Serialize non-empty secrets back to TOML formatted string for persistence.
    pub fn to_toml_string(&self) -> String {
        let lock = match self.secrets.read() {
            Ok(l) => l,
            Err(_) => return String::new(),
        };

        let mut api_keys = Vec::new();
        let mut database = Vec::new();
        let mut custom = Vec::new();

        for (k, v) in lock.iter() {
            if k.contains('.') {
                continue; // Skip namespaced aliases in export
            }
            if k.ends_with("_key") || k.ends_with("_token") || k.contains("api") {
                api_keys.push((k, v));
            } else if k.contains("database") || k.contains("postgres") || k.contains("url") {
                database.push((k, v));
            } else {
                custom.push((k, v));
            }
        }

        api_keys.sort_by_key(|a| a.0);
        database.sort_by_key(|a| a.0);
        custom.sort_by_key(|a| a.0);

        let mut out = String::from("# Companion Secrets Vault\n# NEVER commit this file to source control.\n\n[api_keys]\n");
        for (k, v) in api_keys {
            out.push_str(&format!("{k} = \"{v}\"\n"));
        }

        out.push_str("\n[database]\n");
        for (k, v) in database {
            out.push_str(&format!("{k} = \"{v}\"\n"));
        }

        out.push_str("\n[custom]\n");
        for (k, v) in custom {
            out.push_str(&format!("{k} = \"{v}\"\n"));
        }

        out
    }
}
