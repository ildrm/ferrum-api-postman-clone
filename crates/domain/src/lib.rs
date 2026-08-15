//! Infrastructure-independent domain models for Ferrum API.

use std::{fmt, path::PathBuf, str::FromStr, time::Duration};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

/// Stable identifier for a workspace.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceId(pub Uuid);

/// Stable identifier for a collection.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct CollectionId(pub Uuid);

/// Stable identifier for a saved request.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct RequestId(pub Uuid);

/// Stable identifier for an environment.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct EnvironmentId(pub Uuid);

macro_rules! impl_id {
    ($name:ident) => {
        impl $name {
            /// Creates a locally unique identifier.
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }
    };
}

impl_id!(WorkspaceId);
impl_id!(CollectionId);
impl_id!(RequestId);
impl_id!(EnvironmentId);

/// An HTTP method, including methods created by the user.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum HttpMethod {
    /// GET.
    Get,
    /// POST.
    Post,
    /// PUT.
    Put,
    /// PATCH.
    Patch,
    /// DELETE.
    Delete,
    /// HEAD.
    Head,
    /// OPTIONS.
    Options,
    /// Any valid extension method.
    Custom(String),
}

impl HttpMethod {
    /// Returns the wire representation.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
            Self::Head => "HEAD",
            Self::Options => "OPTIONS",
            Self::Custom(method) => method,
        }
    }
}

impl fmt::Display for HttpMethod {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for HttpMethod {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let method = value.trim().to_ascii_uppercase();
        if method.is_empty()
            || !method
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte == b'-')
        {
            return Err(DomainError::InvalidMethod(value.to_owned()));
        }
        Ok(match method.as_str() {
            "GET" => Self::Get,
            "POST" => Self::Post,
            "PUT" => Self::Put,
            "PATCH" => Self::Patch,
            "DELETE" => Self::Delete,
            "HEAD" => Self::Head,
            "OPTIONS" => Self::Options,
            _ => Self::Custom(method),
        })
    }
}

/// An ordered, optionally enabled key/value editor row.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct KeyValue {
    /// Whether this row is included during execution.
    pub enabled: bool,
    /// Header or parameter name.
    pub key: String,
    /// Header or parameter value.
    pub value: String,
}

impl Default for KeyValue {
    fn default() -> Self {
        Self {
            enabled: true,
            key: String::new(),
            value: String::new(),
        }
    }
}

impl KeyValue {
    /// Creates an enabled key/value row.
    pub fn enabled(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            enabled: true,
            key: key.into(),
            value: value.into(),
        }
    }
}

/// Request body configuration.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum RequestBody {
    /// No body.
    #[default]
    None,
    /// JSON body with `application/json` semantics.
    Json(String),
    /// Plain or custom content type text body.
    Text {
        /// MIME content type.
        content_type: String,
        /// UTF-8 body text.
        content: String,
    },
}

impl RequestBody {
    /// Returns the body content without allocating.
    pub fn content(&self) -> Option<&str> {
        match self {
            Self::None => None,
            Self::Json(content) | Self::Text { content, .. } => Some(content),
        }
    }
}

/// A saved HTTP request aggregate.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedRequest {
    /// Stable identifier.
    pub id: RequestId,
    /// Owning workspace.
    pub workspace_id: WorkspaceId,
    /// Optional owning collection.
    pub collection_id: Option<CollectionId>,
    /// Display name.
    pub name: String,
    /// HTTP method.
    pub method: HttpMethod,
    /// URL template.
    pub url: String,
    /// Ordered query parameters.
    pub query: Vec<KeyValue>,
    /// Ordered request headers.
    pub headers: Vec<KeyValue>,
    /// Request body.
    pub body: RequestBody,
    /// Last local modification time.
    pub updated_at: DateTime<Utc>,
}

impl SavedRequest {
    /// Creates a blank GET request.
    pub fn blank(workspace_id: WorkspaceId) -> Self {
        Self {
            id: RequestId::new(),
            workspace_id,
            collection_id: None,
            name: "Untitled request".into(),
            method: HttpMethod::Get,
            url: String::new(),
            query: vec![KeyValue::default()],
            headers: vec![KeyValue::default()],
            body: RequestBody::None,
            updated_at: Utc::now(),
        }
    }
}

/// A hierarchical request collection.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Collection {
    /// Stable identifier.
    pub id: CollectionId,
    /// Owning workspace.
    pub workspace_id: WorkspaceId,
    /// Optional parent collection/folder.
    pub parent_id: Option<CollectionId>,
    /// Display name.
    pub name: String,
    /// Markdown description.
    pub description: String,
}

/// A local workspace.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Workspace {
    /// Stable identifier.
    pub id: WorkspaceId,
    /// Display name.
    pub name: String,
}

/// A variable value and its sharing/security metadata.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Variable {
    /// Variable name without braces.
    pub name: String,
    /// Local current value for non-sensitive variables.
    pub current_value: String,
    /// Optional shareable initial value.
    pub initial_value: Option<String>,
    /// Whether current value is stored in the OS credential vault.
    pub sensitive: bool,
    /// Whether this variable participates in resolution.
    pub enabled: bool,
}

/// A named environment.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Environment {
    /// Stable identifier.
    pub id: EnvironmentId,
    /// Owning workspace.
    pub workspace_id: WorkspaceId,
    /// Display name.
    pub name: String,
    /// Ordered variables.
    pub variables: Vec<Variable>,
}

/// A request after all local variables have been resolved.
#[derive(Clone, Debug, PartialEq)]
pub struct ExecutableRequest {
    /// Source request identifier when saved.
    pub source_id: Option<RequestId>,
    /// HTTP method.
    pub method: HttpMethod,
    /// Absolute URL before query rows are appended.
    pub url: String,
    /// Enabled, resolved query parameters.
    pub query: Vec<KeyValue>,
    /// Enabled, resolved headers.
    pub headers: Vec<KeyValue>,
    /// Resolved body.
    pub body: RequestBody,
    /// End-to-end timeout.
    pub timeout: Duration,
}

/// A bounded-memory response body.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResponseBody {
    /// Beginning of the body retained for display.
    pub preview: Vec<u8>,
    /// Complete streamed body when it exceeded the preview limit.
    pub file_path: Option<PathBuf>,
    /// Total bytes received.
    pub size: u64,
    /// Whether the preview omits trailing bytes.
    pub truncated: bool,
}

/// Completed HTTP response metadata and body.
#[derive(Clone, Debug, PartialEq)]
pub struct HttpResponse {
    /// Numeric HTTP status.
    pub status: u16,
    /// Canonical status text when known.
    pub status_text: String,
    /// Response headers in received order.
    pub headers: Vec<KeyValue>,
    /// Response content type.
    pub content_type: Option<String>,
    /// Total elapsed request duration.
    pub duration: Duration,
    /// Streamed response body.
    pub body: ResponseBody,
}

/// One persisted, redacted execution record.
#[derive(Clone, Debug, PartialEq)]
pub struct HistoryEntry {
    /// Stable row identifier.
    pub id: Uuid,
    /// Owning workspace.
    pub workspace_id: WorkspaceId,
    /// Optional source request.
    pub request_id: Option<RequestId>,
    /// HTTP method.
    pub method: HttpMethod,
    /// Resolved request URL.
    pub url: String,
    /// Redacted request headers.
    pub request_headers: Vec<KeyValue>,
    /// Response status, absent for network failures.
    pub status: Option<u16>,
    /// Duration in milliseconds.
    pub duration_ms: Option<u64>,
    /// Safe error message for failures.
    pub error: Option<String>,
    /// Execution timestamp.
    pub created_at: DateTime<Utc>,
}

/// Errors caused by invalid domain input.
#[derive(Debug, Error, Eq, PartialEq)]
pub enum DomainError {
    /// HTTP method contains invalid token characters.
    #[error("invalid HTTP method: {0}")]
    InvalidMethod(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_standard_and_custom_methods() {
        assert_eq!("post".parse(), Ok(HttpMethod::Post));
        assert_eq!("PURGE".parse(), Ok(HttpMethod::Custom("PURGE".to_owned())));
        assert!(HttpMethod::from_str("bad method").is_err());
    }

    #[test]
    fn blank_request_has_editable_rows() {
        let request = SavedRequest::blank(WorkspaceId::new());
        assert_eq!(request.method, HttpMethod::Get);
        assert_eq!(request.query.len(), 1);
        assert_eq!(request.headers.len(), 1);
    }
}
