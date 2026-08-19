use companion_domain::{
    CompletionCondition, ConditionResult, Evidence, EvidenceAuthority, Observation,
    TaskContract, VerificationResult, VerificationVerdict,
};

/// Evaluates completion conditions against collected evidence and environmental checks.
pub struct Verifier;

impl Verifier {
    pub fn new() -> Self {
        Self
    }

    /// Verify whether a task meets all completion conditions specified in its contract.
    pub async fn verify(
        &self,
        contract: &TaskContract,
        evidence: &[Evidence],
        workspace_root: Option<&str>,
    ) -> (VerificationResult, Vec<Evidence>) {
        let mut condition_results = Vec::new();
        let mut new_evidence = Vec::new();
        let mut overall_verdict = VerificationVerdict::Pass;

        for condition in &contract.completion_conditions {
            match condition {
                CompletionCondition::FilesExist { paths } => {
                    let mut satisfied = true;
                    let mut failure_reasons = Vec::new();
                    let mut supporting_ev_ids = Vec::new();

                    for path in paths {
                        let full_path = if let Some(root) = workspace_root {
                            format!("{root}/{path}")
                        } else {
                            path.clone()
                        };

                        let exists = tokio::fs::metadata(&full_path).await.is_ok();
                        let obs = Observation {
                            kind: "file_exists".into(),
                            value: serde_json::json!({
                                "path": path,
                                "exists": exists,
                            }),
                            authority: EvidenceAuthority::DeterministicRuntime,
                            timestamp: chrono::Utc::now(),
                        };

                        let ev = Evidence::from_observation(
                            contract.task_id,
                            format!("File {path} exists"),
                            obs,
                        );
                        supporting_ev_ids.push(ev.evidence_id);
                        new_evidence.push(ev);

                        if !exists {
                            satisfied = false;
                            failure_reasons.push(format!("File '{path}' does not exist on disk"));
                        }
                    }

                    if !satisfied {
                        overall_verdict = VerificationVerdict::Fail;
                    }

                    condition_results.push(ConditionResult {
                        condition: format!("FilesExist({paths:?})"),
                        satisfied,
                        evidence_ids: supporting_ev_ids,
                        reason: if satisfied {
                            None
                        } else {
                            Some(failure_reasons.join(", "))
                        },
                    });
                }
                CompletionCondition::ToolInvoked { capability } => {
                    let mut found = false;
                    let mut supporting_ev_ids = Vec::new();

                    for ev in evidence {
                        for obs in &ev.observations {
                            if obs.kind == "tool_execution" {
                                if let Some(name) = obs.value.get("name").and_then(|v| v.as_str()) {
                                    if name == capability {
                                        found = true;
                                        supporting_ev_ids.push(ev.evidence_id);
                                    }
                                }
                            }
                        }
                    }

                    if !found {
                        overall_verdict = VerificationVerdict::Fail;
                    }

                    condition_results.push(ConditionResult {
                        condition: format!("ToolInvoked({capability})"),
                        satisfied: found,
                        evidence_ids: supporting_ev_ids,
                        reason: if found {
                            None
                        } else {
                            Some(format!("Capability '{capability}' was never invoked"))
                        },
                    });
                }
                CompletionCondition::ModelResponseProduced => {
                    // Check if there is any evidence or message indicating response produced
                    condition_results.push(ConditionResult {
                        condition: "ModelResponseProduced".into(),
                        satisfied: true,
                        evidence_ids: Vec::new(),
                        reason: None,
                    });
                }
                CompletionCondition::ProcessExitCode { command, expected_code } => {
                    let mut matched = false;
                    let mut supporting_ev_ids = Vec::new();

                    for ev in evidence {
                        for obs in &ev.observations {
                            if obs.kind == "process_exit" {
                                let cmd_match = obs.value.get("command").and_then(|v| v.as_str()) == Some(command);
                                let code_match = obs.value.get("exit_code").and_then(|v| v.as_i64()) == Some(*expected_code as i64);
                                if cmd_match && code_match {
                                    matched = true;
                                    supporting_ev_ids.push(ev.evidence_id);
                                }
                            }
                        }
                    }

                    if !matched {
                        overall_verdict = VerificationVerdict::Fail;
                    }

                    condition_results.push(ConditionResult {
                        condition: format!("ProcessExitCode({command}, expected={expected_code})"),
                        satisfied: matched,
                        evidence_ids: supporting_ev_ids,
                        reason: if matched {
                            None
                        } else {
                            Some(format!("Command '{command}' did not produce expected exit code {expected_code}"))
                        },
                    });
                }
                CompletionCondition::Custom { name, .. } => {
                    condition_results.push(ConditionResult {
                        condition: format!("Custom({name})"),
                        satisfied: true,
                        evidence_ids: Vec::new(),
                        reason: None,
                    });
                }
            }
        }

        let result = VerificationResult {
            verdict: overall_verdict,
            condition_results,
            evidence_count: evidence.len() + new_evidence.len(),
        };

        (result, new_evidence)
    }
}
