use async_trait::async_trait;
use sqlx::PgPool;
use uuid::Uuid;

use companion_domain::{EventStoreError, TaskId};
use companion_events::{EventStore, TaskEvent};

/// PostgreSQL implementation of the EventStore trait.
pub struct PgEventStore {
    pool: PgPool,
}

impl PgEventStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl EventStore for PgEventStore {
    async fn append(&self, event: TaskEvent) -> Result<(), EventStoreError> {
        let event_type_str = event.event_type.to_string();

        sqlx::query(
            r#"
            INSERT INTO task_events (event_id, task_id, correlation_id, timestamp, sequence, event_type, payload)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(Uuid::from(*event.event_id.as_uuid()))
        .bind(Uuid::from(*event.task_id.as_uuid()))
        .bind(Uuid::from(*event.correlation_id.as_uuid()))
        .bind(event.timestamp)
        .bind(event.sequence)
        .bind(&event_type_str)
        .bind(&event.payload)
        .execute(&self.pool)
        .await
        .map_err(|e| EventStoreError::AppendFailed(e.to_string()))?;

        Ok(())
    }

    async fn append_batch(&self, events: Vec<TaskEvent>) -> Result<(), EventStoreError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| EventStoreError::ConnectionError(e.to_string()))?;

        for event in &events {
            let event_type_str = event.event_type.to_string();

            sqlx::query(
                r#"
                INSERT INTO task_events (event_id, task_id, correlation_id, timestamp, sequence, event_type, payload)
                VALUES ($1, $2, $3, $4, $5, $6, $7)
                "#,
            )
            .bind(Uuid::from(*event.event_id.as_uuid()))
            .bind(Uuid::from(*event.task_id.as_uuid()))
            .bind(Uuid::from(*event.correlation_id.as_uuid()))
            .bind(event.timestamp)
            .bind(event.sequence)
            .bind(&event_type_str)
            .bind(&event.payload)
            .execute(&mut *tx)
            .await
            .map_err(|e| EventStoreError::AppendFailed(e.to_string()))?;
        }

        tx.commit()
            .await
            .map_err(|e| EventStoreError::AppendFailed(e.to_string()))?;

        Ok(())
    }

    async fn load_events(&self, task_id: TaskId) -> Result<Vec<TaskEvent>, EventStoreError> {
        let rows = sqlx::query_as::<_, EventRow>(
            r#"
            SELECT event_id, task_id, correlation_id, timestamp, sequence, event_type, payload
            FROM task_events
            WHERE task_id = $1
            ORDER BY sequence ASC
            "#,
        )
        .bind(Uuid::from(*task_id.as_uuid()))
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EventStoreError::LoadFailed(e.to_string()))?;

        rows.into_iter()
            .map(|row| row.try_into())
            .collect::<Result<Vec<_>, _>>()
    }

    async fn load_events_since(
        &self,
        task_id: TaskId,
        after_sequence: i64,
    ) -> Result<Vec<TaskEvent>, EventStoreError> {
        let rows = sqlx::query_as::<_, EventRow>(
            r#"
            SELECT event_id, task_id, correlation_id, timestamp, sequence, event_type, payload
            FROM task_events
            WHERE task_id = $1 AND sequence > $2
            ORDER BY sequence ASC
            "#,
        )
        .bind(Uuid::from(*task_id.as_uuid()))
        .bind(after_sequence)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EventStoreError::LoadFailed(e.to_string()))?;

        rows.into_iter()
            .map(|row| row.try_into())
            .collect::<Result<Vec<_>, _>>()
    }

    async fn latest_sequence(&self, task_id: TaskId) -> Result<Option<i64>, EventStoreError> {
        let result: Option<(i64,)> = sqlx::query_as(
            r#"
            SELECT MAX(sequence) FROM task_events WHERE task_id = $1
            "#,
        )
        .bind(Uuid::from(*task_id.as_uuid()))
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| EventStoreError::LoadFailed(e.to_string()))?;

        Ok(result.map(|(seq,)| seq))
    }
}

// ---------------------------------------------------------------------------
// Row mapping
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct EventRow {
    event_id: Uuid,
    task_id: Uuid,
    correlation_id: Uuid,
    timestamp: chrono::DateTime<chrono::Utc>,
    sequence: i64,
    event_type: String,
    payload: serde_json::Value,
}

impl TryFrom<EventRow> for TaskEvent {
    type Error = EventStoreError;

    fn try_from(row: EventRow) -> Result<Self, Self::Error> {
        let event_type: companion_events::TaskEventType =
            serde_json::from_value(serde_json::Value::String(row.event_type.clone()))
                .map_err(|e| {
                    EventStoreError::LoadFailed(format!(
                        "invalid event type '{}': {e}",
                        row.event_type
                    ))
                })?;

        Ok(TaskEvent {
            event_id: companion_domain::EventId::from_uuid(row.event_id),
            task_id: companion_domain::TaskId::from_uuid(row.task_id),
            correlation_id: companion_domain::CorrelationId::from_uuid(row.correlation_id),
            timestamp: row.timestamp,
            sequence: row.sequence,
            event_type,
            payload: row.payload,
        })
    }
}
