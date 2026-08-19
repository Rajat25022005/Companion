use serde::{Deserialize, Serialize};

/// Represents the user's profile parsed from `user.md`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserProfile {
    pub raw_markdown: String,
    pub name: Option<String>,
    pub handle: Option<String>,
    pub timezone: Option<String>,
    pub preferences: Vec<String>,
    pub current_projects: Vec<String>,
    pub notes: Option<String>,
}

impl Default for UserProfile {
    fn default() -> Self {
        Self {
            raw_markdown: String::new(),
            name: None,
            handle: None,
            timezone: None,
            preferences: Vec::new(),
            current_projects: Vec::new(),
            notes: None,
        }
    }
}

impl UserProfile {
    /// Parse markdown into a structured UserProfile.
    pub fn from_markdown(content: &str) -> Self {
        let mut profile = Self {
            raw_markdown: content.to_string(),
            ..Default::default()
        };

        let mut current_section = "";
        let mut notes_lines = Vec::new();

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
                                profile.name = Some(val);
                            } else if key == "handle" {
                                profile.handle = Some(val);
                            } else if key == "timezone" {
                                profile.timezone = Some(val);
                            }
                        }
                    }
                }
                "preferences" => {
                    if let Some(pref) = trimmed.strip_prefix("- ") {
                        profile.preferences.push(pref.trim().to_string());
                    }
                }
                "current projects" | "projects" => {
                    if let Some(proj) = trimmed.strip_prefix("- ") {
                        profile.current_projects.push(proj.trim().to_string());
                    }
                }
                "notes" => {
                    if let Some(note) = trimmed.strip_prefix("- ") {
                        notes_lines.push(note.trim().to_string());
                    } else if !trimmed.starts_with('#') {
                        notes_lines.push(trimmed.to_string());
                    }
                }
                _ => {}
            }
        }

        if !notes_lines.is_empty() {
            profile.notes = Some(notes_lines.join("\n"));
        }

        profile
    }

    /// Primary display name of the user.
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or("User")
    }

    /// Formats the profile into a clean context block for LLM system prompt injection.
    pub fn as_context_block(&self) -> String {
        if self.raw_markdown.trim().is_empty() {
            return String::new();
        }

        let mut out = String::from("### User Profile & Preferences:\n");
        if let Some(name) = &self.name {
            out.push_str(&format!("- User Name: {name}\n"));
        }
        if let Some(handle) = &self.handle {
            out.push_str(&format!("- Handle: {handle}\n"));
        }
        if let Some(tz) = &self.timezone {
            out.push_str(&format!("- Timezone: {tz}\n"));
        }
        if !self.preferences.is_empty() {
            out.push_str("- Preferences:\n");
            for p in &self.preferences {
                out.push_str(&format!("  * {p}\n"));
            }
        }
        if !self.current_projects.is_empty() {
            out.push_str("- Current Projects:\n");
            for proj in &self.current_projects {
                out.push_str(&format!("  * {proj}\n"));
            }
        }
        if let Some(notes) = &self.notes {
            out.push_str(&format!("- Notes: {notes}\n"));
        }

        out.trim_end().to_string()
    }
}
