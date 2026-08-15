//! Durable normalized `SQLite` storage for local Ferrum workspaces.

use std::{path::Path, str::FromStr};

use chrono::{DateTime, Utc};
use ferrum_domain::{
    Collection, CollectionId, Environment, EnvironmentId, HistoryEntry, KeyValue, RequestBody,
    RequestId, SavedRequest, Variable, Workspace, WorkspaceId,
};
use sqlx::{
    Row, Sqlite, SqlitePool, Transaction,
    migrate::MigrateError,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use thiserror::Error;
use url::Url;
use uuid::Uuid;

/// `SQLite` database and aggregate repository.
#[derive(Clone, Debug)]
pub struct SqliteStore {
    pool: SqlitePool,
}

impl SqliteStore {
    /// Opens or creates a durable database and runs forward migrations.
    pub async fn open(path: &Path) -> Result<Self, StorageError> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .busy_timeout(std::time::Duration::from_secs(5));
        Self::connect(options, 5).await
    }

    /// Opens an isolated in-memory database.
    pub async fn in_memory() -> Result<Self, StorageError> {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")?
            .foreign_keys(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Memory);
        Self::connect(options, 1).await
    }

    async fn connect(
        options: SqliteConnectOptions,
        max_connections: u32,
    ) -> Result<Self, StorageError> {
        let pool = SqlitePoolOptions::new()
            .max_connections(max_connections)
            .connect_with(options)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }

    /// Returns the oldest workspace, creating the local default atomically if necessary.
    pub async fn ensure_default_workspace(&self) -> Result<Workspace, StorageError> {
        if let Some(row) =
            sqlx::query("SELECT id, name FROM workspaces ORDER BY created_at LIMIT 1")
                .fetch_optional(&self.pool)
                .await?
        {
            return Ok(Workspace {
                id: WorkspaceId(parse_uuid(row.try_get("id")?)?),
                name: row.try_get("name")?,
            });
        }
        let workspace = Workspace {
            id: WorkspaceId::new(),
            name: "Local workspace".into(),
        };
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT OR IGNORE INTO workspaces (id, name, created_at, updated_at) VALUES (?, ?, ?, ?)",
        )
        .bind(workspace.id.to_string())
        .bind(&workspace.name)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(workspace)
    }

    /// Lists collections in stable tree order.
    pub async fn list_collections(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<Collection>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, workspace_id, parent_id, name, description FROM collections WHERE workspace_id = ? ORDER BY parent_id, position, name",
        )
        .bind(workspace_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(collection_from_row).collect()
    }

    /// Creates a collection or nested folder.
    pub async fn create_collection(
        &self,
        workspace_id: WorkspaceId,
        parent_id: Option<CollectionId>,
        name: &str,
    ) -> Result<Collection, StorageError> {
        let name = validate_name(name)?;
        let collection = Collection {
            id: CollectionId::new(),
            workspace_id,
            parent_id,
            name: name.to_owned(),
            description: String::new(),
        };
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO collections (id, workspace_id, parent_id, name, description, position, created_at, updated_at) VALUES (?, ?, ?, ?, '', (SELECT COUNT(*) FROM collections WHERE workspace_id = ? AND parent_id IS ?), ?, ?)",
        )
        .bind(collection.id.to_string())
        .bind(workspace_id.to_string())
        .bind(parent_id.map(|id| id.to_string()))
        .bind(&collection.name)
        .bind(workspace_id.to_string())
        .bind(parent_id.map(|id| id.to_string()))
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(collection)
    }

    /// Atomically inserts or replaces a request and its ordered editor rows.
    pub async fn save_request(&self, request: &SavedRequest) -> Result<(), StorageError> {
        validate_name(&request.name)?;
        let (body_kind, body_content_type, body_content) = body_columns(&request.body);
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO requests (id, workspace_id, collection_id, name, method, url, body_kind, body_content_type, body_content, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(id) DO UPDATE SET collection_id=excluded.collection_id, name=excluded.name, method=excluded.method, url=excluded.url, body_kind=excluded.body_kind, body_content_type=excluded.body_content_type, body_content=excluded.body_content, updated_at=excluded.updated_at",
        )
        .bind(request.id.to_string())
        .bind(request.workspace_id.to_string())
        .bind(request.collection_id.map(|id| id.to_string()))
        .bind(&request.name)
        .bind(request.method.as_str())
        .bind(&request.url)
        .bind(body_kind)
        .bind(body_content_type)
        .bind(body_content)
        .bind(request.updated_at.to_rfc3339())
        .bind(request.updated_at.to_rfc3339())
        .execute(&mut *transaction)
        .await?;
        replace_rows(
            &mut transaction,
            "request_query_params",
            request.id,
            &request.query,
        )
        .await?;
        replace_rows(
            &mut transaction,
            "request_headers",
            request.id,
            &request.headers,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Lists all requests in a workspace with complete query/header aggregates.
    pub async fn list_requests(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<SavedRequest>, StorageError> {
        let ids = sqlx::query(
            "SELECT id FROM requests WHERE workspace_id = ? ORDER BY updated_at DESC, name",
        )
        .bind(workspace_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        let mut requests = Vec::with_capacity(ids.len());
        for row in ids {
            let id = RequestId(parse_uuid(row.try_get("id")?)?);
            requests.push(self.load_request(id).await?);
        }
        Ok(requests)
    }

    /// Loads one request aggregate.
    pub async fn load_request(&self, id: RequestId) -> Result<SavedRequest, StorageError> {
        let row = sqlx::query(
            "SELECT id, workspace_id, collection_id, name, method, url, body_kind, body_content_type, body_content, updated_at FROM requests WHERE id = ?",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?
        .ok_or(StorageError::NotFound)?;
        let query = load_rows(&self.pool, "request_query_params", id).await?;
        let headers = load_rows(&self.pool, "request_headers", id).await?;
        request_from_row(&row, query, headers)
    }

    /// Atomically inserts or replaces an environment and its variables.
    pub async fn save_environment(&self, environment: &Environment) -> Result<(), StorageError> {
        validate_name(&environment.name)?;
        let mut transaction = self.pool.begin().await?;
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO environments (id, workspace_id, name, created_at, updated_at) VALUES (?, ?, ?, ?, ?) ON CONFLICT(id) DO UPDATE SET name=excluded.name, updated_at=excluded.updated_at",
        )
        .bind(environment.id.to_string())
        .bind(environment.workspace_id.to_string())
        .bind(&environment.name)
        .bind(&now)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
        sqlx::query("DELETE FROM variables WHERE environment_id = ?")
            .bind(environment.id.to_string())
            .execute(&mut *transaction)
            .await?;
        for (position, variable) in environment
            .variables
            .iter()
            .filter(|variable| !variable.name.trim().is_empty())
            .enumerate()
        {
            let secret_reference = variable
                .sensitive
                .then(|| format!("environment/{}/{}", environment.id, variable.name));
            sqlx::query(
                "INSERT INTO variables (environment_id, position, name, current_value, initial_value, sensitive, enabled, secret_reference) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(environment.id.to_string())
            .bind(i64::try_from(position).map_err(|_| StorageError::TooManyRows)?)
            .bind(&variable.name)
            .bind((!variable.sensitive).then_some(variable.current_value.as_str()))
            .bind(&variable.initial_value)
            .bind(variable.sensitive)
            .bind(variable.enabled)
            .bind(secret_reference)
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    /// Lists environments and ordered variables.
    pub async fn list_environments(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<Environment>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, workspace_id, name FROM environments WHERE workspace_id = ? ORDER BY name",
        )
        .bind(workspace_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        let mut environments = Vec::with_capacity(rows.len());
        for row in rows {
            let id = EnvironmentId(parse_uuid(row.try_get("id")?)?);
            let variables = sqlx::query(
                "SELECT name, current_value, initial_value, sensitive, enabled FROM variables WHERE environment_id = ? ORDER BY position",
            )
            .bind(id.to_string())
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .map(|variable| {
                Ok(Variable {
                    name: variable.try_get("name")?,
                    current_value: variable.try_get::<Option<String>, _>("current_value")?.unwrap_or_default(),
                    initial_value: variable.try_get("initial_value")?,
                    sensitive: variable.try_get("sensitive")?,
                    enabled: variable.try_get("enabled")?,
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()?;
            environments.push(Environment {
                id,
                workspace_id: WorkspaceId(parse_uuid(row.try_get("workspace_id")?)?),
                name: row.try_get("name")?,
                variables,
            });
        }
        Ok(environments)
    }

    /// Persists one immutable history record after defensive redaction.
    pub async fn append_history(&self, entry: &HistoryEntry) -> Result<(), StorageError> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO history (id, workspace_id, request_id, method, url, status, duration_ms, error, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(entry.id.to_string())
        .bind(entry.workspace_id.to_string())
        .bind(entry.request_id.map(|id| id.to_string()))
        .bind(entry.method.as_str())
        .bind(redact_url(&entry.url))
        .bind(entry.status)
        .bind(entry.duration_ms.and_then(|value| i64::try_from(value).ok()))
        .bind(&entry.error)
        .bind(entry.created_at.to_rfc3339())
        .execute(&mut *transaction)
        .await?;
        for (position, header) in redact_headers(&entry.request_headers).iter().enumerate() {
            sqlx::query(
                "INSERT INTO history_headers (history_id, position, key, value) VALUES (?, ?, ?, ?)",
            )
            .bind(entry.id.to_string())
            .bind(i64::try_from(position).map_err(|_| StorageError::TooManyRows)?)
            .bind(&header.key)
            .bind(&header.value)
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    /// Lists recent history, newest first.
    pub async fn list_history(
        &self,
        workspace_id: WorkspaceId,
        limit: u32,
    ) -> Result<Vec<HistoryEntry>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, request_id, method, url, status, duration_ms, error, created_at FROM history WHERE workspace_id = ? ORDER BY created_at DESC LIMIT ?",
        )
        .bind(workspace_id.to_string())
        .bind(i64::from(limit.min(1_000)))
        .fetch_all(&self.pool)
        .await?;
        let mut history = Vec::with_capacity(rows.len());
        for row in rows {
            let id_text: String = row.try_get("id")?;
            let headers = sqlx::query(
                "SELECT key, value FROM history_headers WHERE history_id = ? ORDER BY position",
            )
            .bind(&id_text)
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .map(|header| {
                Ok(KeyValue::enabled(
                    header.try_get::<String, _>("key")?,
                    header.try_get::<String, _>("value")?,
                ))
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()?;
            let duration: Option<i64> = row.try_get("duration_ms")?;
            history.push(HistoryEntry {
                id: parse_uuid(id_text)?,
                workspace_id,
                request_id: row
                    .try_get::<Option<String>, _>("request_id")?
                    .map(parse_uuid)
                    .transpose()?
                    .map(RequestId),
                method: row.try_get::<String, _>("method")?.parse()?,
                url: row.try_get("url")?,
                request_headers: headers,
                status: row
                    .try_get::<Option<i64>, _>("status")?
                    .and_then(|value| u16::try_from(value).ok()),
                duration_ms: duration.and_then(|value| u64::try_from(value).ok()),
                error: row.try_get("error")?,
                created_at: parse_datetime(row.try_get("created_at")?)?,
            });
        }
        Ok(history)
    }
}

async fn replace_rows(
    transaction: &mut Transaction<'_, Sqlite>,
    table: &'static str,
    request_id: RequestId,
    rows: &[KeyValue],
) -> Result<(), StorageError> {
    let delete = format!("DELETE FROM {table} WHERE request_id = ?");
    sqlx::query(&delete)
        .bind(request_id.to_string())
        .execute(&mut **transaction)
        .await?;
    let insert = format!(
        "INSERT INTO {table} (request_id, position, enabled, key, value) VALUES (?, ?, ?, ?, ?)"
    );
    for (position, row) in rows.iter().enumerate() {
        sqlx::query(&insert)
            .bind(request_id.to_string())
            .bind(i64::try_from(position).map_err(|_| StorageError::TooManyRows)?)
            .bind(row.enabled)
            .bind(&row.key)
            .bind(&row.value)
            .execute(&mut **transaction)
            .await?;
    }
    Ok(())
}

async fn load_rows(
    pool: &SqlitePool,
    table: &'static str,
    request_id: RequestId,
) -> Result<Vec<KeyValue>, StorageError> {
    let query =
        format!("SELECT enabled, key, value FROM {table} WHERE request_id = ? ORDER BY position");
    Ok(sqlx::query(&query)
        .bind(request_id.to_string())
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|row| {
            Ok(KeyValue {
                enabled: row.try_get("enabled")?,
                key: row.try_get("key")?,
                value: row.try_get("value")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?)
}

fn collection_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<Collection, StorageError> {
    Ok(Collection {
        id: CollectionId(parse_uuid(row.try_get("id")?)?),
        workspace_id: WorkspaceId(parse_uuid(row.try_get("workspace_id")?)?),
        parent_id: row
            .try_get::<Option<String>, _>("parent_id")?
            .map(parse_uuid)
            .transpose()?
            .map(CollectionId),
        name: row.try_get("name")?,
        description: row.try_get("description")?,
    })
}

fn request_from_row(
    row: &sqlx::sqlite::SqliteRow,
    query: Vec<KeyValue>,
    headers: Vec<KeyValue>,
) -> Result<SavedRequest, StorageError> {
    let body_kind: String = row.try_get("body_kind")?;
    let content = row
        .try_get::<Option<String>, _>("body_content")?
        .unwrap_or_default();
    let body = match body_kind.as_str() {
        "none" => RequestBody::None,
        "json" => RequestBody::Json(content),
        "text" => RequestBody::Text {
            content_type: row
                .try_get::<Option<String>, _>("body_content_type")?
                .unwrap_or_else(|| "text/plain".into()),
            content,
        },
        other => return Err(StorageError::InvalidBodyKind(other.to_owned())),
    };
    Ok(SavedRequest {
        id: RequestId(parse_uuid(row.try_get("id")?)?),
        workspace_id: WorkspaceId(parse_uuid(row.try_get("workspace_id")?)?),
        collection_id: row
            .try_get::<Option<String>, _>("collection_id")?
            .map(parse_uuid)
            .transpose()?
            .map(CollectionId),
        name: row.try_get("name")?,
        method: row.try_get::<String, _>("method")?.parse()?,
        url: row.try_get("url")?,
        query,
        headers,
        body,
        updated_at: parse_datetime(row.try_get("updated_at")?)?,
    })
}

fn body_columns(body: &RequestBody) -> (&'static str, Option<&str>, Option<&str>) {
    match body {
        RequestBody::None => ("none", None, None),
        RequestBody::Json(content) => ("json", Some("application/json"), Some(content)),
        RequestBody::Text {
            content_type,
            content,
        } => ("text", Some(content_type), Some(content)),
    }
}

fn validate_name(name: &str) -> Result<&str, StorageError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        Err(StorageError::EmptyName)
    } else {
        Ok(trimmed)
    }
}

#[allow(clippy::needless_pass_by_value)]
fn parse_uuid(value: String) -> Result<Uuid, StorageError> {
    Uuid::parse_str(&value).map_err(StorageError::InvalidUuid)
}

#[allow(clippy::needless_pass_by_value)]
fn parse_datetime(value: String) -> Result<DateTime<Utc>, StorageError> {
    DateTime::parse_from_rfc3339(&value)
        .map(|date| date.with_timezone(&Utc))
        .map_err(StorageError::InvalidDate)
}

/// Returns whether a header name commonly carries credentials or session material.
pub fn is_sensitive_header(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    matches!(
        name.as_str(),
        "authorization" | "proxy-authorization" | "cookie" | "set-cookie" | "x-api-key"
    ) || name.contains("token")
        || name.contains("secret")
        || name.contains("password")
        || name.ends_with("-key")
}

/// Replaces credential-bearing header values before storage or logging.
pub fn redact_headers(headers: &[KeyValue]) -> Vec<KeyValue> {
    headers
        .iter()
        .map(|header| KeyValue {
            enabled: header.enabled,
            key: header.key.clone(),
            value: if is_sensitive_header(&header.key) {
                "[REDACTED]".into()
            } else {
                header.value.clone()
            },
        })
        .collect()
}

/// Redacts secret-like URL query parameter values while preserving URL structure.
pub fn redact_url(value: &str) -> String {
    let Ok(mut url) = Url::parse(value) else {
        return "[INVALID URL]".into();
    };
    let pairs = url
        .query_pairs()
        .map(|(key, value)| {
            let redacted = if is_sensitive_header(&key) {
                "[REDACTED]".to_owned()
            } else {
                value.into_owned()
            };
            (key.into_owned(), redacted)
        })
        .collect::<Vec<_>>();
    url.set_query(None);
    if !pairs.is_empty() {
        url.query_pairs_mut().extend_pairs(pairs);
    }
    if !url.username().is_empty() || url.password().is_some() {
        let _ignored = url.set_username("[REDACTED]");
        let _ignored = url.set_password(Some("[REDACTED]"));
    }
    url.to_string()
}

/// Persistence and migration failures.
#[derive(Debug, Error)]
pub enum StorageError {
    /// SQL operation failed.
    #[error("the local database operation failed")]
    Sql(#[from] sqlx::Error),
    /// Database migration failed.
    #[error("the local database migration failed")]
    Migration(#[from] MigrateError),
    /// Stored identifier is corrupt.
    #[error("the local database contains an invalid identifier")]
    InvalidUuid(#[source] uuid::Error),
    /// Stored timestamp is corrupt.
    #[error("the local database contains an invalid timestamp")]
    InvalidDate(#[source] chrono::ParseError),
    /// Stored request body discriminator is unsupported.
    #[error("the local database contains an invalid body kind '{0}'")]
    InvalidBodyKind(String),
    /// Requested aggregate does not exist.
    #[error("the requested local resource was not found")]
    NotFound,
    /// Display names cannot be blank.
    #[error("a resource name cannot be empty")]
    EmptyName,
    /// Editor row count cannot be represented in `SQLite`.
    #[error("the resource contains too many rows")]
    TooManyRows,
    /// HTTP method in storage is invalid.
    #[error(transparent)]
    Domain(#[from] ferrum_domain::DomainError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_credentials_case_insensitively() {
        let headers = vec![
            KeyValue::enabled("Authorization", "Bearer private"),
            KeyValue::enabled("Accept", "application/json"),
        ];
        let redacted = redact_headers(&headers);
        assert_eq!(redacted[0].value, "[REDACTED]");
        assert_eq!(redacted[1].value, "application/json");
        let url = redact_url("https://user:password@example.test/path?api_token=private&page=2");
        assert!(!url.contains("private"));
        assert!(!url.contains("password@example"));
        assert!(url.contains("page=2"));
    }

    #[tokio::test]
    async fn migrates_and_round_trips_aggregates() {
        let store = SqliteStore::in_memory().await.unwrap();
        let workspace = store.ensure_default_workspace().await.unwrap();
        let collection = store
            .create_collection(workspace.id, None, "Accounts")
            .await
            .unwrap();
        let mut request = SavedRequest::blank(workspace.id);
        request.name = "List accounts".into();
        request.url = "https://example.test/accounts".into();
        request.collection_id = Some(collection.id);
        request.query = vec![KeyValue::enabled("page", "1")];
        request.headers = vec![KeyValue::enabled("Accept", "application/json")];
        request.body = RequestBody::Json("{}".into());
        store.save_request(&request).await.unwrap();
        let loaded = store.load_request(request.id).await.unwrap();
        assert_eq!(loaded, request);

        let environment = Environment {
            id: EnvironmentId::new(),
            workspace_id: workspace.id,
            name: "Development".into(),
            variables: vec![Variable {
                name: "base_url".into(),
                current_value: "https://dev.example.test".into(),
                initial_value: None,
                sensitive: false,
                enabled: true,
            }],
        };
        let mut editor_environment = environment.clone();
        editor_environment.variables.push(Variable {
            name: String::new(),
            current_value: String::new(),
            initial_value: None,
            sensitive: false,
            enabled: true,
        });
        store.save_environment(&editor_environment).await.unwrap();
        assert_eq!(
            store.list_environments(workspace.id).await.unwrap(),
            [environment]
        );
    }
}
