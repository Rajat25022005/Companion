use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tracing::info;

use companion_domain::{PolicyRuleId, RiskLevel, TaskContract, TenantId};

// ---------------------------------------------------------------------------
// Policy Condition — Composable rule predicates
// ---------------------------------------------------------------------------

/// A composable predicate that evaluates against task context.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PolicyCondition {
    /// Matches when task risk level is at least the specified level.
    RiskLevelAtLeast { level: RiskLevel },
    /// Matches when a specific capability is being used.
    CapabilityUsed { capability: String },
    /// Matches when the task belongs to a specific tenant.
    TenantEquals { tenant_id: TenantId },
    /// Matches when a specific action string is being performed.
    ActionEquals { action: String },
    /// Logical AND of multiple conditions.
    And { conditions: Vec<PolicyCondition> },
    /// Logical OR of multiple conditions.
    Or { conditions: Vec<PolicyCondition> },
    /// Logical NOT of a condition.
    Not { condition: Box<PolicyCondition> },
    /// Always matches.
    Always,
}

impl PolicyCondition {
    /// Evaluate this condition against task context.
    pub fn matches(&self, contract: &TaskContract, action: &str) -> bool {
        match self {
            Self::RiskLevelAtLeast { level } => {
                risk_level_ord(&contract.risk_level) >= risk_level_ord(level)
            }
            Self::CapabilityUsed { capability } => {
                contract.allowed_tools.iter().any(|t| t == capability)
            }
            Self::TenantEquals { tenant_id } => contract.tenant_id == *tenant_id,
            Self::ActionEquals { action: a } => action == a,
            Self::And { conditions } => {
                conditions.iter().all(|c| c.matches(contract, action))
            }
            Self::Or { conditions } => {
                conditions.iter().any(|c| c.matches(contract, action))
            }
            Self::Not { condition } => !condition.matches(contract, action),
            Self::Always => true,
        }
    }
}

fn risk_level_ord(level: &RiskLevel) -> u8 {
    match level {
        RiskLevel::None => 0,
        RiskLevel::Low => 1,
        RiskLevel::Medium => 2,
        RiskLevel::High => 3,
        RiskLevel::Critical => 4,
    }
}

// ---------------------------------------------------------------------------
// Policy Effect
// ---------------------------------------------------------------------------

/// The effect of a matched policy rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "effect", rename_all = "snake_case")]
pub enum PolicyEffect {
    /// Allow the action to proceed.
    Allow,
    /// Deny the action with a reason.
    Deny { reason: String },
    /// Require HITL approval before proceeding.
    RequireApproval { timeout_secs: u64 },
    /// Allow but log to audit trail only.
    AuditOnly,
}

// ---------------------------------------------------------------------------
// Policy Rule
// ---------------------------------------------------------------------------

/// A declarative policy rule definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyRule {
    pub rule_id: PolicyRuleId,
    pub name: String,
    pub description: String,
    pub condition: PolicyCondition,
    pub effect: PolicyEffect,
    /// Higher priority rules are evaluated first. Equal priority uses insertion order.
    pub priority: u32,
    pub active: bool,
}

// ---------------------------------------------------------------------------
// Policy Decision
// ---------------------------------------------------------------------------

/// The final decision from the policy evaluator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyDecision {
    pub matched_rule: Option<String>,
    pub effect: PolicyEffect,
}

// ---------------------------------------------------------------------------
// Policy Evaluator
// ---------------------------------------------------------------------------

/// Evaluates declarative policy rules against task context.
///
/// Rules are evaluated in priority order (highest first). The first matching
/// rule determines the effect. If no rules match, the default effect is `Allow`.
pub struct PolicyEvaluator {
    rules: Vec<PolicyRule>,
}

impl PolicyEvaluator {
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// Add a policy rule. Rules are re-sorted by priority on insertion.
    pub fn add_rule(&mut self, rule: PolicyRule) {
        info!(
            rule_id = %rule.rule_id,
            name = %rule.name,
            priority = rule.priority,
            "policy rule added"
        );
        self.rules.push(rule);
        self.rules.sort_by(|a, b| b.priority.cmp(&a.priority));
    }

    /// Load policy rules from a JSON string.
    pub fn load_rules_from_json(&mut self, json: &str) -> Result<usize, serde_json::Error> {
        let rules: Vec<PolicyRule> = serde_json::from_str(json)?;
        let count = rules.len();
        for rule in rules {
            self.add_rule(rule);
        }
        Ok(count)
    }

    /// Evaluate all active rules against the given task contract and action.
    ///
    /// Returns the effect of the first matching rule, or `Allow` if none match.
    pub fn evaluate(&self, contract: &TaskContract, action: &str) -> PolicyDecision {
        for rule in &self.rules {
            if !rule.active {
                continue;
            }

            if rule.condition.matches(contract, action) {
                info!(
                    rule = %rule.name,
                    action = %action,
                    effect = ?rule.effect,
                    "policy rule matched"
                );
                return PolicyDecision {
                    matched_rule: Some(rule.name.clone()),
                    effect: rule.effect.clone(),
                };
            }
        }

        // Default: allow
        PolicyDecision {
            matched_rule: None,
            effect: PolicyEffect::Allow,
        }
    }

    /// List all rules (active and inactive).
    pub fn list_rules(&self) -> &[PolicyRule] {
        &self.rules
    }

    /// Count of active rules.
    pub fn active_rule_count(&self) -> usize {
        self.rules.iter().filter(|r| r.active).count()
    }
}

// ---------------------------------------------------------------------------
// Data Residency Guard
// ---------------------------------------------------------------------------

/// Enforces data residency constraints by restricting data flow
/// to permitted regions per tenant.
pub struct DataResidencyGuard {
    allowed_regions: HashMap<TenantId, Vec<String>>,
}

impl DataResidencyGuard {
    pub fn new() -> Self {
        Self {
            allowed_regions: HashMap::new(),
        }
    }

    /// Set allowed regions for a tenant.
    pub fn set_allowed_regions(&mut self, tenant_id: TenantId, regions: Vec<String>) {
        self.allowed_regions.insert(tenant_id, regions);
    }

    /// Check if data processing in the target region is permitted for the tenant.
    pub fn check_residency(
        &self,
        tenant_id: &TenantId,
        target_region: &str,
    ) -> Result<(), String> {
        match self.allowed_regions.get(tenant_id) {
            Some(regions) => {
                if regions.iter().any(|r| r == target_region) {
                    Ok(())
                } else {
                    Err(format!(
                        "Data residency violation: tenant {} is not permitted to process data in region '{}'. Allowed regions: {:?}",
                        tenant_id, target_region, regions
                    ))
                }
            }
            None => {
                // No restrictions configured — allow by default
                Ok(())
            }
        }
    }
}
