use serde_json::Value;

/// Filter and mask secrets, credentials, API keys, and PII in prompts, logs, and tool payloads.
#[derive(Clone)]
pub struct SecurityRedactor {
    mask_emails: bool,
    custom_secrets: Vec<String>,
}

impl SecurityRedactor {
    pub fn new() -> Self {
        Self {
            mask_emails: true,
            custom_secrets: Vec::new(),
        }
    }

    pub fn with_email_masking(mut self, enabled: bool) -> Self {
        self.mask_emails = enabled;
        self
    }

    pub fn with_secrets(mut self, secrets: Vec<String>) -> Self {
        self.custom_secrets = secrets;
        self
    }

    /// Redact known secret and credential patterns from text.
    pub fn redact(&self, input: &str) -> String {
        let mut text = input.to_string();

        // 0. Dynamic Vault Secrets
        for secret in &self.custom_secrets {
            let s = secret.trim();
            if s.len() > 3 {
                text = text.replace(s, "[REDACTED_VAULT_SECRET]");
            }
        }

        // 1. Private Keys
        if text.contains("-----BEGIN") && text.contains("PRIVATE KEY-----") {
            let start = text.find("-----BEGIN").unwrap();
            if let Some(end) = text[start..].find("-----END") {
                if let Some(line_end) = text[start + end..].find('\n') {
                    let total_end = start + end + line_end;
                    text.replace_range(start..total_end, "[REDACTED_PRIVATE_KEY]");
                }
            }
        }

        // 2. JWTs (eyJh...)
        let delimiters = |c: char| c.is_whitespace() || c == '=' || c == ':' || c == '"' || c == '\'' || c == ',' || c == ';' || c == '(' || c == ')' || c == '{' || c == '}';
        let tokens: Vec<String> = text
            .split(delimiters)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();

        for token in &tokens {
            let clean = token.trim_matches(|c: char| !c.is_alphanumeric() && c != '.' && c != '-' && c != '_');
            if clean.starts_with("eyJ") && clean.matches('.').count() == 2 && clean.len() > 30 {
                text = text.replace(clean, "[REDACTED_JWT]");
            }
        }

        // 3. API Key Patterns
        for token in &tokens {
            let clean = token.trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_');
            if clean.starts_with("sk-") && clean.len() >= 20 {
                text = text.replace(clean, "[REDACTED_API_KEY]");
            } else if clean.starts_with("ghp_") && clean.len() >= 30 {
                text = text.replace(clean, "[REDACTED_GITHUB_TOKEN]");
            } else if clean.starts_with("AKIA") && clean.len() == 20 {
                text = text.replace(clean, "[REDACTED_AWS_KEY]");
            } else if clean.starts_with("AIza") && clean.len() == 39 {
                text = text.replace(clean, "[REDACTED_GCP_KEY]");
            }
        }

        // 4. Credit Card Numbers (13-16 consecutive digits with optional dashes)
        let words: Vec<String> = text.split_whitespace().map(|s| s.to_string()).collect();
        for word in words {
            let digits_only: String = word.chars().filter(|c| c.is_ascii_digit()).collect();
            if (13..=19).contains(&digits_only.len()) && (word.contains('-') || digits_only.len() == 16) {
                text = text.replace(&word, "[REDACTED_CREDIT_CARD]");
            }
        }

        // 5. Emails (if configured)
        if self.mask_emails {
            let words: Vec<String> = text.split_whitespace().map(|s| s.to_string()).collect();
            for word in words {
                let clean = word.trim_matches(|c: char| !c.is_alphanumeric() && c != '@' && c != '.');
                if clean.contains('@') && clean.contains('.') {
                    if let Some((user, domain)) = clean.split_once('@') {
                        if !user.is_empty() && !domain.is_empty() {
                            let masked_user = if user.len() <= 2 {
                                format!("{}***", &user[..1])
                            } else {
                                format!("{}***", &user[..2])
                            };
                            let masked_email = format!("{masked_user}@{domain}");
                            text = text.replace(clean, &masked_email);
                        }
                    }
                }
            }
        }

        text
    }

    /// Recursively sanitize JSON objects.
    pub fn redact_json(&self, val: &Value) -> Value {
        match val {
            Value::String(s) => Value::String(self.redact(s)),
            Value::Array(arr) => Value::Array(arr.iter().map(|item| self.redact_json(item)).collect()),
            Value::Object(map) => {
                let mut new_map = serde_json::Map::new();
                for (k, v) in map {
                    let key_lower = k.to_lowercase();
                    if key_lower.contains("password")
                        || key_lower.contains("secret")
                        || key_lower.contains("api_key")
                        || key_lower.contains("token")
                        || key_lower.contains("authorization")
                    {
                        new_map.insert(k.clone(), Value::String("[REDACTED_SECRET]".into()));
                    } else {
                        new_map.insert(k.clone(), self.redact_json(v));
                    }
                }
                Value::Object(new_map)
            }
            _ => val.clone(),
        }
    }

    /// Check if a text contains potential secrets or unmasked credentials.
    pub fn contains_sensitive_data(&self, text: &str) -> bool {
        let redacted = self.redact(text);
        redacted != text
    }
}

impl Default for SecurityRedactor {
    fn default() -> Self {
        Self::new()
    }
}
