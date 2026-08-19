use companion_domain::{IntentClassification, Mode, ModeProfile};

/// Deterministic keyword and tag-based intent parser.
#[derive(Debug, Clone, Default)]
pub struct IntentParser;

impl IntentParser {
    pub fn new() -> Self {
        Self
    }

    /// Parse raw user input into an IntentClassification.
    pub fn parse(&self, input: &str) -> IntentClassification {
        let trimmed = input.trim();
        let mut modes = Vec::new();
        let mut raw_tags = Vec::new();
        let mut remaining_words = Vec::new();

        for word in trimmed.split_whitespace() {
            if word.starts_with('#') {
                if let Some(mode) = Mode::from_tag(word) {
                    modes.push(mode);
                    raw_tags.push(word.to_string());
                    continue;
                }
            }
            remaining_words.push(word);
        }

        let message = remaining_words.join(" ");

        // Default to Ask if no mode tag was provided
        if modes.is_empty() {
            // Check for simple keyword heuristics if no explicit #tag
            let lower = trimmed.to_lowercase();
            if lower.starts_with("create ") || lower.starts_with("write ") || lower.starts_with("build ") {
                modes.push(Mode::Build);
            } else if lower.starts_with("fix ") || lower.starts_with("debug ") {
                modes.push(Mode::Debug);
            } else {
                modes.push(Mode::Ask);
            }
        }

        let mode_profile = ModeProfile::from_modes(modes);
        let is_actionable = mode_profile.tools_required;

        IntentClassification {
            mode_profile,
            message: if message.is_empty() { trimmed.to_string() } else { message },
            raw_tags,
            is_actionable,
            confidence: 1.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_build_tag() {
        let parser = IntentParser::new();
        let intent = parser.parse("#build Create hello.txt with content");
        assert_eq!(intent.mode_profile.primary, Mode::Build);
        assert!(intent.mode_profile.tools_required);
        assert_eq!(intent.message, "Create hello.txt with content");
        assert_eq!(intent.raw_tags, vec!["#build"]);
    }

    #[test]
    fn test_parse_ask_default() {
        let parser = IntentParser::new();
        let intent = parser.parse("What is Rust?");
        assert_eq!(intent.mode_profile.primary, Mode::Ask);
        assert!(!intent.mode_profile.tools_required);
        assert_eq!(intent.message, "What is Rust?");
    }
}
