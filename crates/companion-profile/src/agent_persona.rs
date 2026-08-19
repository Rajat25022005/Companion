use serde::{Deserialize, Serialize};

/// Represents the agent's persona and personality parsed from `agent.md`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentPersona {
    pub raw_markdown: String,
    pub name: Option<String>,
    pub role: Option<String>,
    pub traits: Vec<String>,
    pub behavioral_rules: Vec<String>,
    pub tone: Vec<String>,
}

impl Default for AgentPersona {
    fn default() -> Self {
        Self {
            raw_markdown: String::new(),
            name: Some("Companion".into()),
            role: Some("Autonomous AI Pair-Programmer & Enterprise Execution Runtime".into()),
            traits: Vec::new(),
            behavioral_rules: Vec::new(),
            tone: Vec::new(),
        }
    }
}

impl AgentPersona {
    /// Parse markdown into a structured AgentPersona.
    pub fn from_markdown(content: &str) -> Self {
        let mut persona = Self {
            raw_markdown: content.to_string(),
            ..Default::default()
        };

        let mut current_section = "";

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("## ") {
                current_section = trimmed.trim_start_matches("## ").trim();
                continue;
            }

            if trimmed.is_empty() || trimmed.starts_with("<!--") || trimmed.ends_with("-->") {
                continue;
            }

            match current_section.to_lowercase().as_str() {
                "identity" => {
                    if let Some(rest) = trimmed.strip_prefix("- ") {
                        if let Some((k, v)) = rest.split_once(':') {
                            let key = k.trim().trim_matches('*').to_lowercase();
                            let val = v.trim().to_string();
                            if key == "name" {
                                persona.name = Some(val);
                            } else if key == "role" {
                                persona.role = Some(val);
                            }
                        }
                    }
                }
                "personality" | "traits" => {
                    if let Some(t) = trimmed.strip_prefix("- ") {
                        persona.traits.push(t.trim().to_string());
                    }
                }
                "behavioral rules" | "rules" => {
                    if let Some(r) = trimmed.strip_prefix("- ") {
                        persona.behavioral_rules.push(r.trim().to_string());
                    }
                }
                "tone" => {
                    if let Some(tn) = trimmed.strip_prefix("- ") {
                        persona.tone.push(tn.trim().to_string());
                    } else if !trimmed.starts_with('#') {
                        persona.tone.push(trimmed.to_string());
                    }
                }
                _ => {}
            }
        }

        persona
    }

    /// Primary name of the agent persona.
    pub fn name(&self) -> &str {
        self.name.as_deref().unwrap_or("Companion")
    }

    /// Formats the persona into a system prompt prefix defining the agent's character.
    pub fn as_system_prompt_prefix(&self) -> String {
        if self.raw_markdown.trim().is_empty() {
            return String::new();
        }

        let name = self.name();
        let role = self.role.as_deref().unwrap_or("Autonomous AI Agent");

        let mut out = format!("### Agent Persona & Character:\nYou are {name}, an {role}.\n");

        if !self.traits.is_empty() {
            out.push_str("Personality Traits:\n");
            for t in &self.traits {
                out.push_str(&format!("- {t}\n"));
            }
        }

        if !self.tone.is_empty() {
            out.push_str("Tone & Communication Style:\n");
            for tn in &self.tone {
                out.push_str(&format!("- {tn}\n"));
            }
        }

        if !self.behavioral_rules.is_empty() {
            out.push_str("Core Behavioral Rules:\n");
            for r in &self.behavioral_rules {
                out.push_str(&format!("- {r}\n"));
            }
        }

        out.trim_end().to_string()
    }
}
