use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Mode Profile
// ---------------------------------------------------------------------------

/// A mode specifier parsed from user input (e.g., `#build`, `#ask`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Ask,
    Goal,
    Task,
    Plan,
    Research,
    Build,
    Code,
    Debug,
    Review,
    Summary,
    Remember,
    Recall,
    Forget,
    Decide,
    Delegate,
    Monitor,
    Automate,
}

impl Mode {
    /// Parse a mode tag from a string like "#build" or "#ask".
    pub fn from_tag(tag: &str) -> Option<Self> {
        match tag.trim_start_matches('#').to_lowercase().as_str() {
            "ask" => Some(Self::Ask),
            "goal" => Some(Self::Goal),
            "task" => Some(Self::Task),
            "plan" => Some(Self::Plan),
            "research" => Some(Self::Research),
            "build" => Some(Self::Build),
            "code" => Some(Self::Code),
            "debug" => Some(Self::Debug),
            "review" => Some(Self::Review),
            "summary" => Some(Self::Summary),
            "remember" => Some(Self::Remember),
            "recall" => Some(Self::Recall),
            "forget" => Some(Self::Forget),
            "decide" => Some(Self::Decide),
            "delegate" => Some(Self::Delegate),
            "monitor" => Some(Self::Monitor),
            "automate" => Some(Self::Automate),
            _ => None,
        }
    }

    /// Whether this mode requires tool invocation for action tasks.
    pub fn requires_tools(&self) -> bool {
        matches!(
            self,
            Self::Build | Self::Code | Self::Debug | Self::Goal | Self::Automate
        )
    }

    /// Whether this mode creates durable/persistent state.
    pub fn is_persistent(&self) -> bool {
        matches!(
            self,
            Self::Goal | Self::Remember | Self::Monitor | Self::Automate
        )
    }
}

/// Resolved execution profile from one or more modes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModeProfile {
    /// Primary mode.
    pub primary: Mode,

    /// Additional composed modes.
    pub secondary: Vec<Mode>,

    /// Whether tools must be invoked for task completion.
    pub tools_required: bool,

    /// Whether the task creates persistent state (goals, memory, etc.).
    pub persistent: bool,

    /// Whether verification is mandatory after tool execution.
    pub verification_required: bool,
}

impl ModeProfile {
    /// Create a profile from a single mode.
    pub fn from_mode(mode: Mode) -> Self {
        let tools_required = mode.requires_tools();
        let persistent = mode.is_persistent();
        let verification_required = tools_required; // tools → must verify
        Self {
            primary: mode,
            secondary: Vec::new(),
            tools_required,
            persistent,
            verification_required,
        }
    }

    /// Create a profile from multiple composed modes.
    pub fn from_modes(modes: Vec<Mode>) -> Self {
        assert!(!modes.is_empty(), "at least one mode required");
        let primary = modes[0].clone();
        let secondary: Vec<Mode> = modes[1..].to_vec();

        let tools_required = modes.iter().any(|m| m.requires_tools());
        let persistent = modes.iter().any(|m| m.is_persistent());
        let verification_required = tools_required;

        Self {
            primary,
            secondary,
            tools_required,
            persistent,
            verification_required,
        }
    }
}

// ---------------------------------------------------------------------------
// Intent Classification
// ---------------------------------------------------------------------------

/// The result of parsing a user's input into structured intent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentClassification {
    /// The resolved mode profile.
    pub mode_profile: ModeProfile,

    /// The user's message with mode tags stripped.
    pub message: String,

    /// Raw mode tags found in the input (e.g., ["#build"]).
    pub raw_tags: Vec<String>,

    /// Whether the intent implies an action (state change) vs. a query.
    pub is_actionable: bool,

    /// Confidence in the classification (1.0 = deterministic keyword match).
    pub confidence: f64,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mode_from_tag() {
        assert_eq!(Mode::from_tag("#build"), Some(Mode::Build));
        assert_eq!(Mode::from_tag("#ask"), Some(Mode::Ask));
        assert_eq!(Mode::from_tag("#BUILD"), Some(Mode::Build));
        assert_eq!(Mode::from_tag("build"), Some(Mode::Build));
        assert_eq!(Mode::from_tag("#unknown"), None);
    }

    #[test]
    fn test_mode_requires_tools() {
        assert!(Mode::Build.requires_tools());
        assert!(Mode::Code.requires_tools());
        assert!(!Mode::Ask.requires_tools());
        assert!(!Mode::Research.requires_tools());
    }

    #[test]
    fn test_mode_profile_single() {
        let profile = ModeProfile::from_mode(Mode::Build);
        assert!(profile.tools_required);
        assert!(profile.verification_required);
        assert!(!profile.persistent);
    }

    #[test]
    fn test_mode_profile_composed() {
        let profile = ModeProfile::from_modes(vec![Mode::Goal, Mode::Build]);
        assert!(profile.tools_required);
        assert!(profile.persistent);
        assert_eq!(profile.primary, Mode::Goal);
        assert_eq!(profile.secondary, vec![Mode::Build]);
    }
}
