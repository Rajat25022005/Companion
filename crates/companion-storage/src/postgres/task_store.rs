use async_trait::async_trait;
use sqlx::PgPool;
use uuid::Uuid;

use companion_domain::{TaskContract, TaskId, TaskState, TaskStoreError};

/// Trait for persisting task state (materialized view of events).
#[async_trait]
pub trait TaskStore: Send + Sync {
    async fn save(&self, task_id: TaskId, state: &TaskState, contract: &TaskContract)
        -> Result<(), TaskStoreError>;
    async fn update_state(&self, task_id: TaskId, state: &TaskState) -> Result<(), TaskStoreError>;
    async fn get_state(&self, task_id: TaskId) -> Result<TaskState, TaskStoreError>;
    async fn get_contract(&self, task_id: TaskId) -> Result<TaskContract, TaskStoreError>;
}

/// PostgreSQL implementation of TaskStore.
pub struct PgTaskStore {
    pool: PgPool,
}

impl PgTaskStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl TaskStore for PgTaskStore {
    async fn save(
        &self,
        task_id: TaskId,
        state: &TaskState,
        contract: &TaskContract,
    ) -> Result<(), TaskStoreError> {
        let state_json =
            serde_json::to_value(state).map_err(|e| TaskStoreError::SaveFailed(e.to_string()))?;
        let contract_json = serde_json::to_value(contract)
            .map_err(|e| TaskStoreError::SaveFailed(e.to_string()))?;

        sqlx::query(
            r#"
            INSERT INTO tasks (task_id, tenant_id, workspace_id, state, contract, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, NOW(), NOW())
            ON CONFLICT (task_id) DO UPDATE
                SET state = $4, contract = $5, updated_at = NOW()
            "#,
        )
        .bind(Uuid::from(*task_id.as_uuid()))
        .bind(Uuid::from(*contract.tenant_id.as_uuid()))
        .bind(Uuid::from(*contract.workspace_id.as_uuid()))
        .bind(&state_json)
        .bind(&contract_json)
        .execute(&self.pool)
        .await
        .map_err(|e| TaskStoreError::SaveFailed(e.to_string()))?;

        Ok(())
    }

    async fn update_state(
        &self,
        task_id: TaskId,
        state: &TaskState,
    ) -> Result<(), TaskStoreError> {
        let state_json =
            serde_json::to_value(state).map_err(|e| TaskStoreError::SaveFailed(e.to_string()))?;

        let result = sqlx::query(
            r#"
            UPDATE tasks SET state = $2, updated_at = NOW() WHERE task_id = $1
            "#,
        )
        .bind(Uuid::from(*task_id.as_uuid()))
        .bind(&state_json)
        .execute(&self.pool)
        .await
        .map_err(|e| TaskStoreError::SaveFailed(e.to_string()))?;

        if result.rows_affected() == 0 {
            return Err(TaskStoreError::NotFound(task_id.to_string()));
        }

        Ok(())
    }

    async fn get_state(&self, task_id: TaskId) -> Result<TaskState, TaskStoreError> {
        let row: (serde_json::Value,) = sqlx::query_as(
            r#"SELECT state FROM tasks WHERE task_id = $1"#,
        )
        .bind(Uuid::from(*task_id.as_uuid()))
        .fetch_one(&self.pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => TaskStoreError::NotFound(task_id.to_string()),
            other => TaskStoreError::ConnectionError(other.to_string()),
        })?;

        serde_json::from_value(row.0)
            .map_err(|e| TaskStoreError::ConnectionError(format!("deserialize state: {e}")))
    }

    async fn get_contract(&self, task_id: TaskId) -> Result<TaskContract, TaskStoreError> {
        let row: (serde_json::Value,) = sqlx::query_as(
            r#"SELECT contract FROM tasks WHERE task_id = $1"#,
        )
        .bind(Uuid::from(*task_id.as_uuid()))
        .fetch_one(&self.pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => TaskStoreError::NotFound(task_id.to_string()),
            other => TaskStoreError::ConnectionError(other.to_string()),
        })?;

        serde_json::from_value(row.0)
            .map_err(|e| TaskStoreError::ConnectionError(format!("deserialize contract: {e}")))
    }
}
