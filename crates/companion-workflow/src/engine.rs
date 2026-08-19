use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;
use chrono::Utc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

use companion_agents::AgentTeam;
use companion_domain::{
    RuntimeError, StepId, StepState, TaskState, WorkflowDef,
    WorkflowId, WorkflowStateSnapshot, WorkflowStatus,
};

use crate::dag::WorkflowDag;

pub struct WorkflowEngine {
    team: Arc<AgentTeam>,
    checkpoints: RwLock<HashMap<WorkflowId, Vec<WorkflowStateSnapshot>>>,
}

impl WorkflowEngine {
    pub fn new(team: Arc<AgentTeam>) -> Self {
        Self {
            team,
            checkpoints: RwLock::new(HashMap::new()),
        }
    }

    /// Execute a DAG workflow from scratch to completion.
    pub async fn execute(
        &self,
        definition: WorkflowDef,
        workspace_root: Option<String>,
    ) -> Result<WorkflowStateSnapshot, RuntimeError> {
        let workflow_id = definition.workflow_id;
        let dag = Arc::new(WorkflowDag::compile(definition)?);

        let initial_snapshot = WorkflowStateSnapshot {
            workflow_id,
            status: WorkflowStatus::Running,
            step_states: HashMap::new(),
            step_outputs: HashMap::new(),
            sequence: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        self.record_checkpoint(&initial_snapshot).await;
        self.run_dag(dag, initial_snapshot, workspace_root).await
    }

    /// Resume an interrupted or crashed workflow from its checkpoint snapshot.
    pub async fn resume(
        &self,
        definition: WorkflowDef,
        snapshot: WorkflowStateSnapshot,
        workspace_root: Option<String>,
    ) -> Result<WorkflowStateSnapshot, RuntimeError> {
        info!(workflow_id = %snapshot.workflow_id, "resuming workflow from checkpoint");
        let dag = Arc::new(WorkflowDag::compile(definition)?);
        self.run_dag(dag, snapshot, workspace_root).await
    }

    async fn record_checkpoint(&self, snapshot: &WorkflowStateSnapshot) {
        let mut cps = self.checkpoints.write().await;
        cps.entry(snapshot.workflow_id)
            .or_default()
            .push(snapshot.clone());
    }

    pub async fn get_latest_checkpoint(&self, workflow_id: WorkflowId) -> Option<WorkflowStateSnapshot> {
        let cps = self.checkpoints.read().await;
        cps.get(&workflow_id).and_then(|list| list.last().cloned())
    }

    async fn run_dag(
        &self,
        dag: Arc<WorkflowDag>,
        mut snapshot: WorkflowStateSnapshot,
        workspace_root: Option<String>,
    ) -> Result<WorkflowStateSnapshot, RuntimeError> {
        let total_steps = dag.total_steps();
        let mut completed_steps: HashSet<StepId> = snapshot
            .step_states
            .iter()
            .filter(|(_, state)| state.is_completed())
            .map(|(id, _)| *id)
            .collect();

        let mut failed_steps: HashSet<StepId> = snapshot
            .step_states
            .iter()
            .filter(|(_, state)| matches!(state, StepState::Failed { .. }))
            .map(|(id, _)| *id)
            .collect();

        let mut running_steps: HashSet<StepId> = HashSet::new();

        info!(
            workflow_id = %snapshot.workflow_id,
            completed = completed_steps.len(),
            total = total_steps,
            "starting/resuming DAG execution"
        );

        while completed_steps.len() + failed_steps.len() < total_steps {
            // Find all steps whose dependencies have succeeded
            let ready_steps: Vec<_> = dag
                .get_ready_steps(&completed_steps, &running_steps)
                .into_iter()
                .cloned()
                .collect();

            if ready_steps.is_empty() && running_steps.is_empty() {
                // Deadlock or unresolvable failure
                let reason = "Workflow stalled: no ready steps and no running steps".to_string();
                snapshot.status = WorkflowStatus::Failed { reason: reason.clone() };
                snapshot.updated_at = Utc::now();
                snapshot.sequence += 1;
                self.record_checkpoint(&snapshot).await;
                return Ok(snapshot);
            }

            // Launch ready steps concurrently
            for step in ready_steps {
                running_steps.insert(step.step_id);

                let team = self.team.clone();
                let ws_root = workspace_root.clone();
                let step_id = step.step_id;
                let step_prompt = step.prompt.clone();
                let role = step.assigned_role.clone();

                let agent = match team.get_by_role(&role).await {
                    Some(a) => a,
                    None => {
                        let reason = format!("Assigned agent role not found in team: {role}");
                        snapshot.step_states.insert(step_id, StepState::Failed { reason });
                        failed_steps.insert(step_id);
                        running_steps.remove(&step_id);
                        continue;
                    }
                };

                let start_time = Instant::now();
                snapshot.step_states.insert(
                    step_id,
                    StepState::Running {
                        agent_id: agent.address.agent_id,
                        task_id: companion_domain::TaskId::new(),
                        started_at: Utc::now(),
                    },
                );

                // Execute step on assigned agent
                let exec_res = agent.execute_task(&step_prompt, ws_root).await;
                let elapsed_ms = start_time.elapsed().as_millis() as u64;

                running_steps.remove(&step_id);

                match exec_res {
                    Ok((_task_id, state, output)) => {
                        if state == TaskState::Completed {
                            info!(step_id = %step_id, "step completed successfully");
                            completed_steps.insert(step_id);
                            snapshot.step_states.insert(
                                step_id,
                                StepState::Completed {
                                    output: output.clone(),
                                    execution_ms: elapsed_ms,
                                },
                            );
                            snapshot.step_outputs.insert(step_id, output);
                        } else {
                            warn!(step_id = %step_id, state = %state, "step failed");
                            failed_steps.insert(step_id);
                            snapshot.step_states.insert(
                                step_id,
                                StepState::Failed {
                                    reason: format!("Task ended in non-completed state: {state}"),
                                },
                            );
                        }
                    }
                    Err(e) => {
                        error!(step_id = %step_id, error = %e, "step execution error");
                        failed_steps.insert(step_id);
                        snapshot.step_states.insert(
                            step_id,
                            StepState::Failed {
                                reason: e.to_string(),
                            },
                        );
                    }
                }

                // Checkpoint after every step completion
                snapshot.sequence += 1;
                snapshot.updated_at = Utc::now();
                self.record_checkpoint(&snapshot).await;
            }
        }

        if failed_steps.is_empty() {
            snapshot.status = WorkflowStatus::Completed;
        } else {
            snapshot.status = WorkflowStatus::Failed {
                reason: format!("{} step(s) failed in workflow", failed_steps.len()),
            };
        }

        snapshot.sequence += 1;
        snapshot.updated_at = Utc::now();
        self.record_checkpoint(&snapshot).await;

        Ok(snapshot)
    }

    pub fn team(&self) -> &Arc<AgentTeam> {
        &self.team
    }
}
