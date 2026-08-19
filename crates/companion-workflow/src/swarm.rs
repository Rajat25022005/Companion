use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::{RwLock, Semaphore};
use tracing::info;

use companion_domain::{
    AgentRole, RuntimeError, StepId, TaskId, TenantId, WorkflowDef, WorkflowId, WorkflowStep,
};

// ---------------------------------------------------------------------------
// Scheduled Task — Priority Queue Entry
// ---------------------------------------------------------------------------

/// A task entry in the priority scheduler queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledTask {
    pub task_id: TaskId,
    pub tenant_id: TenantId,
    pub priority: u32,
    pub submitted_at: DateTime<Utc>,
    pub prompt: String,
    /// Computed effective priority (higher = dequeued sooner).
    pub effective_priority: f64,
}

impl PartialEq for ScheduledTask {
    fn eq(&self, other: &Self) -> bool {
        self.task_id == other.task_id
    }
}

impl Eq for ScheduledTask {}

impl PartialOrd for ScheduledTask {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ScheduledTask {
    fn cmp(&self, other: &Self) -> Ordering {
        self.effective_priority
            .partial_cmp(&other.effective_priority)
            .unwrap_or(Ordering::Equal)
            .then_with(|| self.submitted_at.cmp(&other.submitted_at).reverse())
    }
}

// ---------------------------------------------------------------------------
// Priority Scheduler — Multi-tenant fair-share weighted task queue
// ---------------------------------------------------------------------------

/// Multi-tenant fair-share priority scheduler.
///
/// Tasks are ordered by `effective_priority = base_priority * tenant_weight`.
/// Higher-weighted tenants get proportionally more dequeue share.
pub struct PriorityScheduler {
    queue: RwLock<BinaryHeap<ScheduledTask>>,
    tenant_weights: RwLock<HashMap<TenantId, f64>>,
}

impl PriorityScheduler {
    pub fn new() -> Self {
        Self {
            queue: RwLock::new(BinaryHeap::new()),
            tenant_weights: RwLock::new(HashMap::new()),
        }
    }

    /// Set the scheduling weight for a tenant (default = 1.0).
    pub async fn set_tenant_weight(&self, tenant_id: TenantId, weight: f64) {
        self.tenant_weights.write().await.insert(tenant_id, weight);
        info!(tenant_id = %tenant_id, weight = weight, "tenant scheduling weight updated");
    }

    /// Enqueue a task with priority and tenant weight applied.
    pub async fn enqueue(&self, mut task: ScheduledTask) {
        let weights = self.tenant_weights.read().await;
        let weight = weights.get(&task.tenant_id).copied().unwrap_or(1.0);
        task.effective_priority = task.priority as f64 * weight;

        info!(
            task_id = %task.task_id,
            tenant_id = %task.tenant_id,
            priority = task.priority,
            effective = task.effective_priority,
            "task enqueued"
        );

        self.queue.write().await.push(task);
    }

    /// Dequeue the highest-priority task.
    pub async fn dequeue(&self) -> Option<ScheduledTask> {
        self.queue.write().await.pop()
    }

    /// Current queue depth.
    pub async fn queue_depth(&self) -> usize {
        self.queue.read().await.len()
    }
}

// ---------------------------------------------------------------------------
// Worker Pool — Elastic concurrency-bounded agent pool
// ---------------------------------------------------------------------------

/// A pool of agent workers bounded by a concurrency semaphore.
pub struct WorkerPool {
    max_concurrency: usize,
    semaphore: Arc<Semaphore>,
    active_tasks: RwLock<Vec<TaskId>>,
    completed_count: RwLock<usize>,
}

impl WorkerPool {
    pub fn new(max_concurrency: usize) -> Self {
        Self {
            max_concurrency,
            semaphore: Arc::new(Semaphore::new(max_concurrency)),
            active_tasks: RwLock::new(Vec::new()),
            completed_count: RwLock::new(0),
        }
    }

    /// Acquire a permit from the pool. Blocks if at capacity.
    pub async fn acquire(&self, task_id: TaskId) -> Result<WorkerPermit, RuntimeError> {
        let permit = self
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|e| RuntimeError::Internal(format!("semaphore closed: {e}")))?;

        self.active_tasks.write().await.push(task_id);

        info!(
            task_id = %task_id,
            active = self.active_count().await,
            max = self.max_concurrency,
            "worker permit acquired"
        );

        Ok(WorkerPermit {
            task_id,
            _permit: permit,
        })
    }

    /// Release a permit (called when task completes).
    pub async fn release(&self, task_id: TaskId) {
        let mut active = self.active_tasks.write().await;
        active.retain(|id| *id != task_id);
        *self.completed_count.write().await += 1;
    }

    /// Number of currently active workers.
    pub async fn active_count(&self) -> usize {
        self.active_tasks.read().await.len()
    }

    /// Total completed task count.
    pub async fn completed_count(&self) -> usize {
        *self.completed_count.read().await
    }

    /// Maximum concurrency limit.
    pub fn max_concurrency(&self) -> usize {
        self.max_concurrency
    }
}

/// A held worker permit. The semaphore slot is released on drop.
pub struct WorkerPermit {
    pub task_id: TaskId,
    _permit: tokio::sync::OwnedSemaphorePermit,
}

// ---------------------------------------------------------------------------
// Swarm Coordinator — Goal decomposition and swarm orchestration
// ---------------------------------------------------------------------------

/// Decomposes complex goals into dynamically generated sub-DAGs
/// and orchestrates execution across the elastic worker pool.
pub struct SwarmCoordinator {
    worker_pool: Arc<WorkerPool>,
    scheduler: Arc<PriorityScheduler>,
}

impl SwarmCoordinator {
    pub fn new(worker_pool: Arc<WorkerPool>, scheduler: Arc<PriorityScheduler>) -> Self {
        Self {
            worker_pool,
            scheduler,
        }
    }

    /// Decompose a high-level goal into a sub-DAG of agent tasks.
    ///
    /// The decomposition creates a 3-phase pipeline:
    /// 1. **Architect** — design phase (analysis & planning)
    /// 2. **Engineer** — implementation phase (build & execute)
    /// 3. **Reviewer** — verification phase (test & validate)
    pub fn decompose_goal(
        &self,
        goal: &str,
        _tenant_id: TenantId,
        available_roles: &[String],
    ) -> Result<WorkflowDef, RuntimeError> {
        if available_roles.is_empty() {
            return Err(RuntimeError::Internal(
                "Cannot decompose goal: no agent roles available".into(),
            ));
        }

        let workflow_id = WorkflowId::new();

        let architect_id = StepId::new();
        let engineer_id = StepId::new();
        let reviewer_id = StepId::new();

        let parse_role = |role_str: &str| -> AgentRole {
            match role_str.to_lowercase().as_str() {
                "coordinator" => AgentRole::Coordinator,
                "architect" => AgentRole::Architect,
                "engineer" => AgentRole::Engineer,
                "reviewer" => AgentRole::Reviewer,
                "researcher" => AgentRole::Researcher,
                custom => AgentRole::Custom(custom.to_string()),
            }
        };

        let default_role = parse_role(&available_roles[0]);

        let steps = vec![
            WorkflowStep {
                step_id: architect_id,
                name: "Architectural Planning".into(),
                description: format!("Analyze and design architecture for {goal}"),
                prompt: format!("Analyze and design an architecture plan for: {goal}"),
                assigned_role: if available_roles.iter().any(|r| r.eq_ignore_ascii_case("architect")) {
                    AgentRole::Architect
                } else {
                    default_role.clone()
                },
                required_tools: vec!["filesystem.read".into(), "filesystem.list".into()],
                retry_policy: Default::default(),
                timeout_secs: 300,
            },
            WorkflowStep {
                step_id: engineer_id,
                name: "Engineering Implementation".into(),
                description: format!("Implement the architectural plan for {goal}"),
                prompt: format!("Implement the design from the architect for: {goal}"),
                assigned_role: if available_roles.iter().any(|r| r.eq_ignore_ascii_case("engineer")) {
                    AgentRole::Engineer
                } else {
                    default_role.clone()
                },
                required_tools: vec![
                    "filesystem.read".into(),
                    "filesystem.write".into(),
                    "filesystem.list".into(),
                    "process.execute".into(),
                ],
                retry_policy: Default::default(),
                timeout_secs: 300,
            },
            WorkflowStep {
                step_id: reviewer_id,
                name: "Quality Review & Verification".into(),
                description: format!("Verify implementation for {goal}"),
                prompt: format!("Review and verify the implementation for: {goal}"),
                assigned_role: if available_roles.iter().any(|r| r.eq_ignore_ascii_case("reviewer")) {
                    AgentRole::Reviewer
                } else {
                    default_role
                },
                required_tools: vec!["filesystem.read".into(), "filesystem.list".into(), "process.execute".into()],
                retry_policy: Default::default(),
                timeout_secs: 300,
            },
        ];

        let dependencies = vec![
            companion_domain::StepDependency {
                from: architect_id,
                to: engineer_id,
            },
            companion_domain::StepDependency {
                from: engineer_id,
                to: reviewer_id,
            },
        ];

        info!(
            workflow_id = %workflow_id,
            steps = steps.len(),
            goal = %goal,
            "goal decomposed into sub-DAG"
        );

        Ok(WorkflowDef {
            workflow_id,
            name: format!("swarm:{}", &goal[..goal.len().min(40)]),
            description: format!("Autonomous swarm execution for: {goal}"),
            steps,
            dependencies,
        })
    }

    /// Submit a task to the priority scheduler queue.
    pub async fn submit_to_queue(
        &self,
        task_id: TaskId,
        tenant_id: TenantId,
        priority: u32,
        prompt: String,
    ) {
        let task = ScheduledTask {
            task_id,
            tenant_id,
            priority,
            submitted_at: Utc::now(),
            prompt,
            effective_priority: 0.0, // Computed by scheduler
        };
        self.scheduler.enqueue(task).await;
    }

    /// Get the worker pool reference.
    pub fn worker_pool(&self) -> &Arc<WorkerPool> {
        &self.worker_pool
    }

    /// Get the scheduler reference.
    pub fn scheduler(&self) -> &Arc<PriorityScheduler> {
        &self.scheduler
    }
}
