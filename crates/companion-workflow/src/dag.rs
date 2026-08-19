use std::collections::{HashMap, HashSet, VecDeque};
use companion_domain::{RuntimeError, StepId, WorkflowDef, WorkflowStep};

/// Validated Directed Acyclic Graph (DAG) for a workflow.
#[derive(Debug)]
pub struct WorkflowDag {
    pub definition: WorkflowDef,
    steps_map: HashMap<StepId, WorkflowStep>,
    /// Adjacency list: step -> downstream steps that depend on it
    downstream: HashMap<StepId, Vec<StepId>>,
    /// Inverted adjacency list: step -> upstream dependencies that must finish first
    upstream: HashMap<StepId, Vec<StepId>>,
}

impl WorkflowDag {
    pub fn compile(definition: WorkflowDef) -> Result<Self, RuntimeError> {
        let mut steps_map = HashMap::new();
        let mut downstream: HashMap<StepId, Vec<StepId>> = HashMap::new();
        let mut upstream: HashMap<StepId, Vec<StepId>> = HashMap::new();

        for step in &definition.steps {
            if steps_map.contains_key(&step.step_id) {
                return Err(RuntimeError::Internal(format!(
                    "Duplicate step_id in workflow: {}",
                    step.step_id
                )));
            }
            steps_map.insert(step.step_id, step.clone());
            downstream.entry(step.step_id).or_default();
            upstream.entry(step.step_id).or_default();
        }

        for dep in &definition.dependencies {
            if !steps_map.contains_key(&dep.from) {
                return Err(RuntimeError::Internal(format!(
                    "Dependency from non-existent step: {}",
                    dep.from
                )));
            }
            if !steps_map.contains_key(&dep.to) {
                return Err(RuntimeError::Internal(format!(
                    "Dependency to non-existent step: {}",
                    dep.to
                )));
            }

            downstream.entry(dep.from).or_default().push(dep.to);
            upstream.entry(dep.to).or_default().push(dep.from);
        }

        // Cycle Detection via Kahn's Algorithm
        let mut in_degrees: HashMap<StepId, usize> = HashMap::new();
        for step_id in steps_map.keys() {
            in_degrees.insert(*step_id, upstream.get(step_id).map(|v| v.len()).unwrap_or(0));
        }

        let mut queue = VecDeque::new();
        for (step_id, &degree) in &in_degrees {
            if degree == 0 {
                queue.push_back(*step_id);
            }
        }

        let mut visited_count = 0;
        while let Some(step_id) = queue.pop_front() {
            visited_count += 1;
            if let Some(next_steps) = downstream.get(&step_id) {
                for next in next_steps {
                    if let Some(deg) = in_degrees.get_mut(next) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push_back(*next);
                        }
                    }
                }
            }
        }

        if visited_count != steps_map.len() {
            return Err(RuntimeError::Internal(
                "Cycle detected in workflow step dependencies".into(),
            ));
        }

        Ok(Self {
            definition,
            steps_map,
            downstream,
            upstream,
        })
    }

    /// Get all steps that have all their upstream dependencies satisfied and are ready to execute.
    pub fn get_ready_steps(
        &self,
        completed_steps: &HashSet<StepId>,
        running_steps: &HashSet<StepId>,
    ) -> Vec<&WorkflowStep> {
        let mut ready = Vec::new();

        for (step_id, step) in &self.steps_map {
            if completed_steps.contains(step_id) || running_steps.contains(step_id) {
                continue;
            }

            let deps = self.upstream.get(step_id).cloned().unwrap_or_default();
            let all_deps_completed = deps.iter().all(|dep| completed_steps.contains(dep));

            if all_deps_completed {
                ready.push(step);
            }
        }

        ready
    }

    pub fn get_step(&self, step_id: &StepId) -> Option<&WorkflowStep> {
        self.steps_map.get(step_id)
    }

    pub fn total_steps(&self) -> usize {
        self.steps_map.len()
    }

    pub fn downstream(&self) -> &HashMap<StepId, Vec<StepId>> {
        &self.downstream
    }
}
