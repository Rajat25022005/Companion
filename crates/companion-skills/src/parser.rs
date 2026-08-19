use companion_domain::{
    ProcedureStep, RuntimeError, Skill, SkillLifecycleState, SkillTrigger,
};

/// Parser and serializer for human-readable SKILL.md interchange files.
pub struct SkillMarkdownParser;

impl SkillMarkdownParser {
    /// Parse a SKILL.md markdown document into a structured Skill IR.
    pub fn parse(markdown: &str) -> Result<Skill, RuntimeError> {
        let trimmed = markdown.trim();

        // 1. Extract YAML Frontmatter if present
        let (frontmatter, body) = if trimmed.starts_with("---") {
            let rest = &trimmed[3..];
            if let Some(end_idx) = rest.find("---") {
                let fm = &rest[..end_idx].trim();
                let b = &rest[end_idx + 3..].trim();
                (*fm, *b)
            } else {
                ("", trimmed)
            }
        } else {
            ("", trimmed)
        };

        let mut name = "unnamed_skill".to_string();
        let mut version = 1u32;
        let mut description = String::new();
        let mut capabilities = Vec::new();
        let mut keywords = Vec::new();
        let mut state = SkillLifecycleState::Candidate;

        // Parse key-value pairs from frontmatter lines
        for line in frontmatter.lines() {
            let line = line.trim();
            if let Some((k, v)) = line.split_once(':') {
                let key = k.trim();
                let val = v.trim();
                match key {
                    "name" => name = val.to_string(),
                    "version" => version = val.parse().unwrap_or(1),
                    "description" => description = val.to_string(),
                    "state" => {
                        state = match val.to_lowercase().as_str() {
                            "active" => SkillLifecycleState::Active,
                            "staged" => SkillLifecycleState::Staged,
                            "canary" => SkillLifecycleState::Canary,
                            "promoted" => SkillLifecycleState::Promoted,
                            "rejected" => SkillLifecycleState::Rejected,
                            "deprecated" => SkillLifecycleState::Deprecated,
                            _ => SkillLifecycleState::Candidate,
                        };
                    }
                    _ => {}
                }
            } else if line.starts_with("- ") {
                let item = line.trim_start_matches("- ").trim();
                if item.contains('.') {
                    capabilities.push(item.to_string());
                } else {
                    keywords.push(item.to_string());
                }
            }
        }

        // Parse procedural steps from markdown body
        let mut steps = Vec::new();
        let mut step_num = 1;
        let mut in_procedure = false;

        for line in body.lines() {
            let trimmed_line = line.trim();
            if trimmed_line.starts_with("## Procedure") {
                in_procedure = true;
                continue;
            } else if trimmed_line.starts_with("## ") {
                in_procedure = false;
            }

            if in_procedure && trimmed_line.starts_with("- ") {
                let content = trimmed_line.trim_start_matches("- ").trim();
                let mut cap = None;
                let mut desc = content.to_string();

                if let Some(open) = content.find('[') {
                    if let Some(close) = content.find(']') {
                        let extracted_cap = &content[open + 1..close];
                        cap = Some(extracted_cap.to_string());
                        desc = content[close + 1..].trim_start_matches(':').trim().to_string();
                    }
                }

                let mut step = ProcedureStep::new(
                    format!("step_{step_num}"),
                    format!("Step {step_num}"),
                    desc,
                );
                if let Some(c) = cap {
                    if !capabilities.contains(&c) {
                        capabilities.push(c.clone());
                    }
                    step = step.with_capability(c);
                }
                steps.push(step);
                step_num += 1;
            }
        }

        if description.is_empty() {
            description = format!("Procedural skill `{name}`");
        }

        let trigger = SkillTrigger {
            intent: Some(name.clone()),
            keywords,
            required_capabilities: capabilities.clone(),
            mode: None,
        };

        let skill = Skill::new(name, version, description)
            .with_steps(steps)
            .with_capabilities(capabilities)
            .with_triggers(vec![trigger])
            .with_state(state);

        Ok(skill)
    }

    /// Serialize a Skill IR into a formatted SKILL.md document string.
    pub fn serialize(skill: &Skill) -> String {
        let mut out = String::new();

        // Frontmatter
        out.push_str("---\n");
        out.push_str(&format!("name: {}\n", skill.name));
        out.push_str(&format!("version: {}\n", skill.version));
        out.push_str(&format!("description: {}\n", skill.description));
        out.push_str(&format!("state: {:?}\n", skill.lifecycle_state));
        if !skill.required_capabilities.is_empty() {
            out.push_str("capabilities:\n");
            for cap in &skill.required_capabilities {
                out.push_str(&format!("  - {}\n", cap));
            }
        }
        out.push_str("---\n\n");

        // Body
        out.push_str(&format!("# {}\n\n", skill.name));
        out.push_str(&format!("{}\n\n", skill.description));

        if !skill.procedure_graph.is_empty() {
            out.push_str("## Procedure\n");
            for step in &skill.procedure_graph {
                if let Some(ref cap) = step.required_capability {
                    out.push_str(&format!("- [{}] {}\n", cap, step.description));
                } else {
                    out.push_str(&format!("- {}\n", step.description));
                }
            }
            out.push('\n');
        }

        if !skill.examples.is_empty() {
            out.push_str("## Examples\n");
            for ex in &skill.examples {
                out.push_str(&format!("### {}\n", ex.title));
                out.push_str(&format!("- Input: {}\n", ex.user_input));
                out.push_str(&format!("- Outcome: {}\n\n", ex.outcome));
            }
        }

        out
    }
}
