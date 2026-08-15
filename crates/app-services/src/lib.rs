//! Application use cases coordinating persistence, variables, secrets, and protocols.

use std::{collections::BTreeSet, sync::Arc, time::Duration};

use chrono::Utc;
use ferrum_domain::{
    Collection, Environment, ExecutableRequest, HistoryEntry, HttpResponse, KeyValue, RequestBody,
    SavedRequest, Workspace,
};
use ferrum_http_client::{HttpError, ProtocolClient};
use ferrum_secrets::{SecretError, SecretStore};
use ferrum_storage::{SqliteStore, StorageError};
use ferrum_variables::{
    ResolveError, Scope, SensitiveValueProvider, VariableResolver, VariableScope,
};
use thiserror::Error;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Data loaded before the first desktop frame.
#[derive(Clone, Debug)]
pub struct AppSnapshot {
    /// Active local workspace.
    pub workspace: Workspace,
    /// Saved collections.
    pub collections: Vec<Collection>,
    /// Saved requests.
    pub requests: Vec<SavedRequest>,
    /// Saved environments.
    pub environments: Vec<Environment>,
    /// Recent redacted request history.
    pub history: Vec<HistoryEntry>,
}

/// Successful request execution and the history row that was persisted.
#[derive(Clone, Debug)]
pub struct ExecutionOutput {
    /// HTTP response.
    pub response: HttpResponse,
    /// Newly persisted history entry.
    pub history: HistoryEntry,
}

/// Main application API consumed by native and future CLI frontends.
#[derive(Clone)]
pub struct FerrumService {
    store: SqliteStore,
    client: Arc<dyn ProtocolClient>,
    secrets: Arc<dyn SecretStore>,
}

impl FerrumService {
    /// Creates an application service from replaceable infrastructure adapters.
    pub fn new(
        store: SqliteStore,
        client: Arc<dyn ProtocolClient>,
        secrets: Arc<dyn SecretStore>,
    ) -> Self {
        Self {
            store,
            client,
            secrets,
        }
    }

    /// Loads all navigation state required for first paint.
    pub async fn initialize(&self) -> Result<AppSnapshot, AppError> {
        let workspace = self.store.ensure_default_workspace().await?;
        let (collections, requests, environments, history) = tokio::try_join!(
            self.store.list_collections(workspace.id),
            self.store.list_requests(workspace.id),
            self.store.list_environments(workspace.id),
            self.store.list_history(workspace.id, 200),
        )?;
        Ok(AppSnapshot {
            workspace,
            collections,
            requests,
            environments,
            history,
        })
    }

    /// Creates one collection.
    pub async fn create_collection(
        &self,
        workspace: &Workspace,
        name: &str,
    ) -> Result<Collection, AppError> {
        Ok(self
            .store
            .create_collection(workspace.id, None, name)
            .await?)
    }

    /// Saves a request aggregate.
    pub async fn save_request(&self, request: &SavedRequest) -> Result<(), AppError> {
        self.store.save_request(request).await?;
        Ok(())
    }

    /// Saves an environment, moving sensitive current values to the OS vault first.
    pub async fn save_environment(&self, environment: &Environment) -> Result<(), AppError> {
        for variable in environment
            .variables
            .iter()
            .filter(|item| item.sensitive && !item.name.trim().is_empty())
        {
            if !variable.current_value.is_empty() {
                self.secrets.set(
                    &secret_key(environment, &variable.name),
                    &variable.current_value,
                )?;
            }
        }
        self.store.save_environment(environment).await?;
        Ok(())
    }

    /// Resolves variables, executes a request, and writes secret-safe history.
    pub async fn execute_request(
        &self,
        workspace: &Workspace,
        request: &SavedRequest,
        environment: Option<&Environment>,
        cancellation: CancellationToken,
    ) -> Result<ExecutionOutput, AppError> {
        // Autosave makes crash/session restoration reliable and gives history a valid source.
        self.store.save_request(request).await?;
        let empty = [];
        let variables = environment.map_or(empty.as_slice(), |item| item.variables.as_slice());
        let provider = EnvironmentSecretProvider {
            environment,
            secrets: self.secrets.as_ref(),
        };
        let resolver = VariableResolver::new(
            [VariableScope {
                scope: Scope::Environment,
                variables,
            }],
            &provider,
        );
        let mut unresolved = BTreeSet::new();
        let url = resolve_text(&resolver, &request.url, &mut unresolved)?;
        let query = resolve_rows(&resolver, &request.query, &mut unresolved)?;
        let headers = resolve_rows(&resolver, &request.headers, &mut unresolved)?;
        let body = match &request.body {
            RequestBody::None => RequestBody::None,
            RequestBody::Json(content) => {
                RequestBody::Json(resolve_text(&resolver, content, &mut unresolved)?)
            }
            RequestBody::Text {
                content_type,
                content,
            } => RequestBody::Text {
                content_type: resolve_text(&resolver, content_type, &mut unresolved)?,
                content: resolve_text(&resolver, content, &mut unresolved)?,
            },
        };
        if !unresolved.is_empty() {
            return Err(AppError::UnresolvedVariables(
                unresolved.into_iter().collect::<Vec<_>>().join(", "),
            ));
        }

        let executable = ExecutableRequest {
            source_id: Some(request.id),
            method: request.method.clone(),
            url: url.clone(),
            query,
            headers: headers.clone(),
            body,
            timeout: Duration::from_secs(60),
        };
        let response = self.client.execute(executable, cancellation).await;
        let history = match &response {
            Ok(response) => HistoryEntry {
                id: Uuid::new_v4(),
                workspace_id: workspace.id,
                request_id: Some(request.id),
                method: request.method.clone(),
                url,
                request_headers: headers,
                status: Some(response.status),
                duration_ms: u64::try_from(response.duration.as_millis()).ok(),
                error: None,
                created_at: Utc::now(),
            },
            Err(error) => HistoryEntry {
                id: Uuid::new_v4(),
                workspace_id: workspace.id,
                request_id: Some(request.id),
                method: request.method.clone(),
                url,
                request_headers: headers,
                status: None,
                duration_ms: None,
                error: Some(error.to_string()),
                created_at: Utc::now(),
            },
        };
        self.store.append_history(&history).await?;
        Ok(ExecutionOutput {
            response: response?,
            history,
        })
    }
}

fn resolve_rows<P: SensitiveValueProvider>(
    resolver: &VariableResolver<'_, P>,
    rows: &[KeyValue],
    unresolved: &mut BTreeSet<String>,
) -> Result<Vec<KeyValue>, ResolveError> {
    rows.iter()
        .map(|row| {
            Ok(KeyValue {
                enabled: row.enabled,
                key: resolve_text(resolver, &row.key, unresolved)?,
                value: resolve_text(resolver, &row.value, unresolved)?,
            })
        })
        .collect()
}

fn resolve_text<P: SensitiveValueProvider>(
    resolver: &VariableResolver<'_, P>,
    input: &str,
    unresolved: &mut BTreeSet<String>,
) -> Result<String, ResolveError> {
    let output = resolver.interpolate(input)?;
    unresolved.extend(output.unresolved);
    Ok(output.value)
}

fn secret_key(environment: &Environment, name: &str) -> String {
    format!("environment/{}/{}", environment.id, name)
}

struct EnvironmentSecretProvider<'a> {
    environment: Option<&'a Environment>,
    secrets: &'a dyn SecretStore,
}

impl SensitiveValueProvider for EnvironmentSecretProvider<'_> {
    fn get(&self, name: &str) -> Result<Option<String>, ResolveError> {
        let Some(environment) = self.environment else {
            return Ok(None);
        };
        self.secrets
            .get(&secret_key(environment, name))
            .map_err(|error| ResolveError::Secret {
                name: name.to_owned(),
                message: error.to_string(),
            })
    }
}

/// Application use-case failures.
#[derive(Debug, Error)]
pub enum AppError {
    /// Persistence failed.
    #[error(transparent)]
    Storage(#[from] StorageError),
    /// HTTP execution failed.
    #[error(transparent)]
    Http(#[from] HttpError),
    /// Secure credential storage failed.
    #[error(transparent)]
    Secret(#[from] SecretError),
    /// Variable resolution failed.
    #[error(transparent)]
    Resolve(#[from] ResolveError),
    /// One or more placeholders lack a value.
    #[error("unresolved variables: {0}")]
    UnresolvedVariables(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use ferrum_domain::{HttpMethod, ResponseBody, Variable};
    use ferrum_secrets::MemorySecretStore;

    struct EchoClient;

    #[async_trait]
    impl ProtocolClient for EchoClient {
        async fn execute(
            &self,
            request: ExecutableRequest,
            _cancellation: CancellationToken,
        ) -> Result<HttpResponse, HttpError> {
            assert_eq!(request.url, "https://example.test/users");
            assert_eq!(request.headers[0].value, "vault-value");
            Ok(HttpResponse {
                status: 200,
                status_text: "OK".into(),
                headers: vec![],
                content_type: Some("application/json".into()),
                duration: Duration::from_millis(4),
                body: ResponseBody {
                    preview: b"{}".to_vec(),
                    file_path: None,
                    size: 2,
                    truncated: false,
                },
            })
        }
    }

    #[tokio::test]
    async fn executes_with_environment_and_vault_variables() {
        let store = SqliteStore::in_memory().await.unwrap();
        let secrets = Arc::new(MemorySecretStore::default());
        let service = FerrumService::new(store, Arc::new(EchoClient), secrets.clone());
        let snapshot = service.initialize().await.unwrap();
        let environment = Environment {
            id: ferrum_domain::EnvironmentId::new(),
            workspace_id: snapshot.workspace.id,
            name: "Test".into(),
            variables: vec![
                Variable {
                    name: "base".into(),
                    current_value: "https://example.test".into(),
                    initial_value: None,
                    sensitive: false,
                    enabled: true,
                },
                Variable {
                    name: "token".into(),
                    current_value: "vault-value".into(),
                    initial_value: None,
                    sensitive: true,
                    enabled: true,
                },
            ],
        };
        service.save_environment(&environment).await.unwrap();
        let mut request = SavedRequest::blank(snapshot.workspace.id);
        request.method = HttpMethod::Get;
        request.url = "{{base}}/users".into();
        request.headers = vec![KeyValue::enabled("Authorization", "{{token}}")];
        let output = service
            .execute_request(
                &snapshot.workspace,
                &request,
                Some(&environment),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(output.response.status, 200);
    }
}
