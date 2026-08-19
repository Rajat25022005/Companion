use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};

/// Serializable snapshot of all collected runtime metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeMetricsSnapshot {
    pub tasks_total: u64,
    pub tasks_succeeded: u64,
    pub tasks_failed: u64,
    pub tasks_repaired: u64,
    pub tool_calls_total: u64,
    pub tool_errors_total: u64,
    pub prompt_tokens_total: u64,
    pub completion_tokens_total: u64,
    pub estimated_cost_usd: f64,
    pub context_compilations_total: u64,
    pub context_cache_hits_total: u64,
    pub skill_executions_total: u64,
    pub skill_rollbacks_total: u64,
    pub tool_calls_by_name: HashMap<String, u64>,
}

/// Thread-safe Prometheus-compatible metrics collector for Companion Enterprise.
#[derive(Clone)]
pub struct MetricsCollector {
    tasks_total: Arc<AtomicU64>,
    tasks_succeeded: Arc<AtomicU64>,
    tasks_failed: Arc<AtomicU64>,
    tasks_repaired: Arc<AtomicU64>,

    tool_calls_total: Arc<AtomicU64>,
    tool_errors_total: Arc<AtomicU64>,
    tool_duration_ms_total: Arc<AtomicU64>,

    prompt_tokens_total: Arc<AtomicU64>,
    completion_tokens_total: Arc<AtomicU64>,
    estimated_cost_cents: Arc<AtomicU64>, // stored as micro-cents for precision

    context_compilations_total: Arc<AtomicU64>,
    context_cache_hits_total: Arc<AtomicU64>,

    skill_executions_total: Arc<AtomicU64>,
    skill_rollbacks_total: Arc<AtomicU64>,

    tool_calls_by_name: Arc<RwLock<HashMap<String, u64>>>,
}

impl MetricsCollector {
    pub fn new() -> Self {
        Self {
            tasks_total: Arc::new(AtomicU64::new(0)),
            tasks_succeeded: Arc::new(AtomicU64::new(0)),
            tasks_failed: Arc::new(AtomicU64::new(0)),
            tasks_repaired: Arc::new(AtomicU64::new(0)),
            tool_calls_total: Arc::new(AtomicU64::new(0)),
            tool_errors_total: Arc::new(AtomicU64::new(0)),
            tool_duration_ms_total: Arc::new(AtomicU64::new(0)),
            prompt_tokens_total: Arc::new(AtomicU64::new(0)),
            completion_tokens_total: Arc::new(AtomicU64::new(0)),
            estimated_cost_cents: Arc::new(AtomicU64::new(0)),
            context_compilations_total: Arc::new(AtomicU64::new(0)),
            context_cache_hits_total: Arc::new(AtomicU64::new(0)),
            skill_executions_total: Arc::new(AtomicU64::new(0)),
            skill_rollbacks_total: Arc::new(AtomicU64::new(0)),
            tool_calls_by_name: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn record_task(&self, success: bool, repaired: bool) {
        self.tasks_total.fetch_add(1, Ordering::Relaxed);
        if success {
            self.tasks_succeeded.fetch_add(1, Ordering::Relaxed);
        } else {
            self.tasks_failed.fetch_add(1, Ordering::Relaxed);
        }
        if repaired {
            self.tasks_repaired.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub async fn record_tool_call(&self, tool: &str, duration_ms: u64, is_error: bool) {
        self.tool_calls_total.fetch_add(1, Ordering::Relaxed);
        self.tool_duration_ms_total.fetch_add(duration_ms, Ordering::Relaxed);
        if is_error {
            self.tool_errors_total.fetch_add(1, Ordering::Relaxed);
        }

        let mut map = self.tool_calls_by_name.write().await;
        *map.entry(tool.to_string()).or_default() += 1;
    }

    pub fn record_tokens(&self, prompt: u64, completion: u64, cost_usd: f64) {
        self.prompt_tokens_total.fetch_add(prompt, Ordering::Relaxed);
        self.completion_tokens_total.fetch_add(completion, Ordering::Relaxed);
        let micro_cents = (cost_usd * 100_000_000.0) as u64;
        self.estimated_cost_cents.fetch_add(micro_cents, Ordering::Relaxed);
    }

    pub fn record_context_compilation(&self, cache_hit: bool) {
        self.context_compilations_total.fetch_add(1, Ordering::Relaxed);
        if cache_hit {
            self.context_cache_hits_total.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn record_skill_execution(&self, rollback: bool) {
        self.skill_executions_total.fetch_add(1, Ordering::Relaxed);
        if rollback {
            self.skill_rollbacks_total.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub async fn snapshot(&self) -> RuntimeMetricsSnapshot {
        let tool_map = self.tool_calls_by_name.read().await.clone();
        let cost = self.estimated_cost_cents.load(Ordering::Relaxed) as f64 / 100_000_000.0;

        RuntimeMetricsSnapshot {
            tasks_total: self.tasks_total.load(Ordering::Relaxed),
            tasks_succeeded: self.tasks_succeeded.load(Ordering::Relaxed),
            tasks_failed: self.tasks_failed.load(Ordering::Relaxed),
            tasks_repaired: self.tasks_repaired.load(Ordering::Relaxed),
            tool_calls_total: self.tool_calls_total.load(Ordering::Relaxed),
            tool_errors_total: self.tool_errors_total.load(Ordering::Relaxed),
            prompt_tokens_total: self.prompt_tokens_total.load(Ordering::Relaxed),
            completion_tokens_total: self.completion_tokens_total.load(Ordering::Relaxed),
            estimated_cost_usd: cost,
            context_compilations_total: self.context_compilations_total.load(Ordering::Relaxed),
            context_cache_hits_total: self.context_cache_hits_total.load(Ordering::Relaxed),
            skill_executions_total: self.skill_executions_total.load(Ordering::Relaxed),
            skill_rollbacks_total: self.skill_rollbacks_total.load(Ordering::Relaxed),
            tool_calls_by_name: tool_map,
        }
    }

    /// Export metrics in Prometheus exposition text format.
    pub async fn export_prometheus(&self) -> String {
        let snap = self.snapshot().await;
        let mut out = String::new();

        out.push_str("# HELP companion_tasks_total Total number of tasks executed\n");
        out.push_str("# TYPE companion_tasks_total counter\n");
        out.push_str(&format!("companion_tasks_total {}\n", snap.tasks_total));

        out.push_str("# HELP companion_tasks_succeeded Number of tasks completed successfully\n");
        out.push_str("# TYPE companion_tasks_succeeded counter\n");
        out.push_str(&format!("companion_tasks_succeeded {}\n", snap.tasks_succeeded));

        out.push_str("# HELP companion_tasks_failed Number of failed tasks\n");
        out.push_str("# TYPE companion_tasks_failed counter\n");
        out.push_str(&format!("companion_tasks_failed {}\n", snap.tasks_failed));

        out.push_str("# HELP companion_tasks_repaired Number of tasks requiring repair turns\n");
        out.push_str("# TYPE companion_tasks_repaired counter\n");
        out.push_str(&format!("companion_tasks_repaired {}\n", snap.tasks_repaired));

        out.push_str("# HELP companion_tool_calls_total Total tool executions\n");
        out.push_str("# TYPE companion_tool_calls_total counter\n");
        out.push_str(&format!("companion_tool_calls_total {}\n", snap.tool_calls_total));

        out.push_str("# HELP companion_tool_errors_total Total tool errors\n");
        out.push_str("# TYPE companion_tool_errors_total counter\n");
        out.push_str(&format!("companion_tool_errors_total {}\n", snap.tool_errors_total));

        out.push_str("# HELP companion_prompt_tokens_total Total prompt tokens consumed\n");
        out.push_str("# TYPE companion_prompt_tokens_total counter\n");
        out.push_str(&format!("companion_prompt_tokens_total {}\n", snap.prompt_tokens_total));

        out.push_str("# HELP companion_completion_tokens_total Total completion tokens generated\n");
        out.push_str("# TYPE companion_completion_tokens_total counter\n");
        out.push_str(&format!("companion_completion_tokens_total {}\n", snap.completion_tokens_total));

        out.push_str("# HELP companion_estimated_cost_usd Estimated model and compute cost in USD\n");
        out.push_str("# TYPE companion_estimated_cost_usd gauge\n");
        out.push_str(&format!("companion_estimated_cost_usd {:.6}\n", snap.estimated_cost_usd));

        out.push_str("# HELP companion_context_compilations_total Total context compilation runs\n");
        out.push_str("# TYPE companion_context_compilations_total counter\n");
        out.push_str(&format!("companion_context_compilations_total {}\n", snap.context_compilations_total));

        out.push_str("# HELP companion_context_cache_hits_total Prefix cache hits\n");
        out.push_str("# TYPE companion_context_cache_hits_total counter\n");
        out.push_str(&format!("companion_context_cache_hits_total {}\n", snap.context_cache_hits_total));

        for (tool, count) in &snap.tool_calls_by_name {
            out.push_str(&format!("companion_tool_invocations_total{{tool=\"{tool}\"}} {count}\n"));
        }

        out
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}
