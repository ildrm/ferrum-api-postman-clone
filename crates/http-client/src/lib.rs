//! Secure-by-default HTTP execution with cancellation and bounded memory usage.

use std::{path::PathBuf, time::Instant};

use async_trait::async_trait;
use ferrum_domain::{ExecutableRequest, HttpResponse, KeyValue, RequestBody, ResponseBody};
use futures_util::StreamExt;
use reqwest::{Client, Method, redirect::Policy};
use thiserror::Error;
use tokio::{fs::File, io::AsyncWriteExt};
use tokio_util::sync::CancellationToken;
use url::Url;
use uuid::Uuid;

/// Default number of body bytes retained in memory for display.
pub const DEFAULT_PREVIEW_LIMIT: usize = 2 * 1024 * 1024;

/// Protocol-client abstraction used by application services and test doubles.
#[async_trait]
pub trait ProtocolClient: Send + Sync {
    /// Executes one request until completion or cancellation.
    async fn execute(
        &self,
        request: ExecutableRequest,
        cancellation: CancellationToken,
    ) -> Result<HttpResponse, HttpError>;
}

/// Production HTTP adapter.
#[derive(Clone)]
pub struct HttpEngine {
    client: Client,
    cache_dir: PathBuf,
    preview_limit: usize,
}

impl HttpEngine {
    /// Builds an engine with pooled connections, rustls TLS, and bounded redirects.
    pub fn new(cache_dir: PathBuf) -> Result<Self, HttpError> {
        let client = Client::builder()
            .redirect(Policy::limited(10))
            .connect_timeout(std::time::Duration::from_secs(15))
            .user_agent(concat!("FerrumAPI/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(HttpError::BuildClient)?;
        Ok(Self {
            client,
            cache_dir,
            preview_limit: DEFAULT_PREVIEW_LIMIT,
        })
    }

    /// Overrides the preview limit, primarily for deterministic tests.
    #[must_use]
    pub fn with_preview_limit(mut self, bytes: usize) -> Self {
        self.preview_limit = bytes.max(1);
        self
    }
}

#[async_trait]
impl ProtocolClient for HttpEngine {
    async fn execute(
        &self,
        request: ExecutableRequest,
        cancellation: CancellationToken,
    ) -> Result<HttpResponse, HttpError> {
        if cancellation.is_cancelled() {
            return Err(HttpError::Cancelled);
        }
        let mut url = Url::parse(&request.url).map_err(HttpError::InvalidUrl)?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(HttpError::UnsupportedScheme(url.scheme().to_owned()));
        }
        {
            let mut pairs = url.query_pairs_mut();
            for row in request.query.iter().filter(|row| row.enabled) {
                if !row.key.is_empty() {
                    pairs.append_pair(&row.key, &row.value);
                }
            }
        }

        let method = Method::from_bytes(request.method.as_str().as_bytes())
            .map_err(|_| HttpError::InvalidMethod)?;
        let mut builder = self.client.request(method, url).timeout(request.timeout);
        for header in request.headers.iter().filter(|row| row.enabled) {
            if !header.key.is_empty() {
                builder = builder.header(&header.key, &header.value);
            }
        }
        builder = match request.body {
            RequestBody::None => builder,
            RequestBody::Json(content) => builder
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(content),
            RequestBody::Text {
                content_type,
                content,
            } => builder
                .header(reqwest::header::CONTENT_TYPE, content_type)
                .body(content),
        };

        let started = Instant::now();
        let response = tokio::select! {
            () = cancellation.cancelled() => return Err(HttpError::Cancelled),
            result = builder.send() => result.map_err(HttpError::Transport)?,
        };
        let status = response.status();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned);
        let headers = response
            .headers()
            .iter()
            .map(|(name, value)| {
                KeyValue::enabled(name.as_str(), value.to_str().unwrap_or("<binary>"))
            })
            .collect();

        tokio::fs::create_dir_all(&self.cache_dir)
            .await
            .map_err(HttpError::CacheIo)?;
        let mut preview = Vec::with_capacity(self.preview_limit.min(64 * 1024));
        let mut file: Option<(PathBuf, File)> = None;
        let mut size = 0_u64;
        let mut stream = response.bytes_stream();

        loop {
            let next = tokio::select! {
                () = cancellation.cancelled() => return Err(HttpError::Cancelled),
                item = stream.next() => item,
            };
            let Some(chunk) = next else { break };
            let chunk = chunk.map_err(HttpError::Transport)?;
            size = size.saturating_add(chunk.len() as u64);

            let remaining = self.preview_limit.saturating_sub(preview.len());
            preview.extend_from_slice(&chunk[..remaining.min(chunk.len())]);
            if size > self.preview_limit as u64 {
                if file.is_none() {
                    let path = self.cache_dir.join(format!("{}.response", Uuid::new_v4()));
                    let mut opened = File::create(&path).await.map_err(HttpError::CacheIo)?;
                    let bytes_before_chunk =
                        usize::try_from(size.saturating_sub(chunk.len() as u64))
                            .unwrap_or(usize::MAX);
                    let prefix_len = bytes_before_chunk.min(preview.len());
                    opened
                        .write_all(&preview[..prefix_len])
                        .await
                        .map_err(HttpError::CacheIo)?;
                    file = Some((path, opened));
                }
                if let Some((_, output)) = file.as_mut() {
                    output.write_all(&chunk).await.map_err(HttpError::CacheIo)?;
                }
            }
        }

        if let Some((_, output)) = file.as_mut() {
            output.flush().await.map_err(HttpError::CacheIo)?;
        }
        tracing::info!(
            status = status.as_u16(),
            response_bytes = size,
            "HTTP request completed"
        );
        Ok(HttpResponse {
            status: status.as_u16(),
            status_text: status.canonical_reason().unwrap_or("Unknown").to_owned(),
            headers,
            content_type,
            duration: started.elapsed(),
            body: ResponseBody {
                preview,
                file_path: file.map(|(path, _)| path),
                size,
                truncated: size > self.preview_limit as u64,
            },
        })
    }
}

/// HTTP execution failures with secret-safe display messages.
#[derive(Debug, Error)]
pub enum HttpError {
    /// Client construction failed.
    #[error("could not initialize the HTTP client")]
    BuildClient(#[source] reqwest::Error),
    /// URL is malformed.
    #[error("the request URL is invalid")]
    InvalidUrl(#[source] url::ParseError),
    /// Only HTTP and HTTPS can be executed by this adapter.
    #[error("unsupported URL scheme '{0}'; use http or https")]
    UnsupportedScheme(String),
    /// Method is not an HTTP token.
    #[error("the HTTP method is invalid")]
    InvalidMethod,
    /// DNS, connection, TLS, timeout, or HTTP transport failure.
    #[error("the network request failed")]
    Transport(#[source] reqwest::Error),
    /// User cancelled the operation.
    #[error("request cancelled")]
    Cancelled,
    /// Response cache could not be written.
    #[error("the streamed response cache could not be written")]
    CacheIo(#[source] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    #[tokio::test]
    async fn rejects_non_http_schemes_before_network_io() {
        let engine = HttpEngine::new(std::env::temp_dir()).unwrap();
        let request = ExecutableRequest {
            source_id: None,
            method: ferrum_domain::HttpMethod::Get,
            url: "file:///private/data".into(),
            query: vec![],
            headers: vec![],
            body: RequestBody::None,
            timeout: std::time::Duration::from_secs(1),
        };
        let result = engine.execute(request, CancellationToken::new()).await;
        assert!(matches!(result, Err(HttpError::UnsupportedScheme(_))));
    }

    #[tokio::test]
    async fn cancellation_wins_before_send() {
        let engine = HttpEngine::new(std::env::temp_dir()).unwrap();
        let token = CancellationToken::new();
        token.cancel();
        let request = ExecutableRequest {
            source_id: None,
            method: ferrum_domain::HttpMethod::Get,
            url: "http://192.0.2.1/".into(),
            query: vec![],
            headers: vec![],
            body: RequestBody::None,
            timeout: std::time::Duration::from_secs(30),
        };
        let result = engine.execute(request, token).await;
        assert!(matches!(result, Err(HttpError::Cancelled)));
    }

    #[tokio::test]
    async fn streams_large_response_and_keeps_bounded_preview() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let body = br#"{"ok":true}"#.to_vec();
        let expected = body.clone();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 2_048];
            let _received = stream.read(&mut request).await.unwrap();
            let headers = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(headers.as_bytes()).await.unwrap();
            stream.write_all(&body).await.unwrap();
        });

        let cache = std::env::temp_dir().join(format!("ferrum-http-test-{}", Uuid::new_v4()));
        let engine = HttpEngine::new(cache).unwrap().with_preview_limit(4);
        let request = ExecutableRequest {
            source_id: None,
            method: ferrum_domain::HttpMethod::Get,
            url: format!("http://{address}/json"),
            query: vec![],
            headers: vec![],
            body: RequestBody::None,
            timeout: std::time::Duration::from_secs(2),
        };
        let response = engine
            .execute(request, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.body.preview, expected[..4]);
        assert!(response.body.truncated);
        let path = response.body.file_path.unwrap();
        assert_eq!(tokio::fs::read(path).await.unwrap(), expected);
    }
}
