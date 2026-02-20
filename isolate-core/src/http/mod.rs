//! HTTP client capability for sandboxed WASM execution.
//!
//! This module provides a secure HTTP client that enforces capability-based
//! access control, allowing sandboxed code to make HTTP requests only to
//! explicitly allowed hosts.
//!
//! # Example
//!
//! ```rust,ignore
//! use isolate_core::http::{HttpClient, HttpRequest, HttpMethod};
//! use isolate_core::capability::Capability;
//!
//! // Create a client with allowed hosts
//! let client = HttpClient::new(vec![
//!     Capability::http_client(vec!["api.example.com"]),
//! ]);
//!
//! // Make a request
//! let request = HttpRequest::get("https://api.example.com/data");
//! let response = client.execute(request).await?;
//! ```

#![allow(missing_docs)]
mod client;
mod request;
mod response;

pub use client::HttpClient;
pub use request::{HttpMethod, HttpRequest, HttpRequestBuilder};
pub use response::{HttpResponse, HttpResponseBody};

use crate::error::Error;

/// HTTP error types.
#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    /// Host not allowed by capability.
    #[error("HTTP request to host '{host}' is not allowed")]
    HostNotAllowed { host: String },

    /// Invalid URL.
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),

    /// Request timeout.
    #[error("Request timed out after {0:?}")]
    Timeout(std::time::Duration),

    /// Connection error.
    #[error("Connection error: {0}")]
    Connection(String),

    /// Request body too large.
    #[error("Request body exceeds maximum size of {max} bytes")]
    RequestBodyTooLarge { max: usize },

    /// Response body too large.
    #[error("Response body exceeds maximum size of {max} bytes")]
    ResponseBodyTooLarge { max: usize },

    /// Too many redirects.
    #[error("Too many redirects (max: {max})")]
    TooManyRedirects { max: usize },

    /// HTTP protocol error.
    #[error("HTTP error: {0}")]
    Http(String),
}

impl From<HttpError> for Error {
    fn from(err: HttpError) -> Self {
        Error::Http(err.to_string())
    }
}

/// HTTP client configuration.
#[derive(Debug, Clone)]
pub struct HttpClientConfig {
    /// Maximum request body size in bytes.
    pub max_request_body: usize,
    /// Maximum response body size in bytes.
    pub max_response_body: usize,
    /// Request timeout.
    pub timeout: std::time::Duration,
    /// Maximum number of redirects to follow.
    pub max_redirects: usize,
    /// User agent string.
    pub user_agent: String,
    /// Whether to allow insecure TLS connections (for testing only).
    pub allow_insecure_tls: bool,
}

impl Default for HttpClientConfig {
    fn default() -> Self {
        Self {
            max_request_body: 10 * 1024 * 1024,   // 10 MB
            max_response_body: 100 * 1024 * 1024, // 100 MB
            timeout: std::time::Duration::from_secs(30),
            max_redirects: 10,
            user_agent: format!("isolate/{} (secure sandbox runtime)", env!("CARGO_PKG_VERSION")),
            allow_insecure_tls: false,
        }
    }
}

impl HttpClientConfig {
    /// Create a new configuration builder.
    pub fn builder() -> HttpClientConfigBuilder {
        HttpClientConfigBuilder::default()
    }
}

/// Builder for HTTP client configuration.
#[derive(Debug, Default)]
pub struct HttpClientConfigBuilder {
    config: HttpClientConfig,
}

impl HttpClientConfigBuilder {
    /// Set maximum request body size.
    pub fn max_request_body(mut self, size: usize) -> Self {
        self.config.max_request_body = size;
        self
    }

    /// Set maximum response body size.
    pub fn max_response_body(mut self, size: usize) -> Self {
        self.config.max_response_body = size;
        self
    }

    /// Set request timeout.
    pub fn timeout(mut self, timeout: std::time::Duration) -> Self {
        self.config.timeout = timeout;
        self
    }

    /// Set maximum redirects.
    pub fn max_redirects(mut self, max: usize) -> Self {
        self.config.max_redirects = max;
        self
    }

    /// Set user agent.
    pub fn user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.config.user_agent = user_agent.into();
        self
    }

    /// Build the configuration.
    pub fn build(self) -> HttpClientConfig {
        self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http_client_config_defaults() {
        let config = HttpClientConfig::default();
        assert_eq!(config.max_request_body, 10 * 1024 * 1024);
        assert_eq!(config.max_response_body, 100 * 1024 * 1024);
        assert_eq!(config.timeout, std::time::Duration::from_secs(30));
        assert_eq!(config.max_redirects, 10);
    }

    #[test]
    fn test_http_client_config_builder() {
        let config = HttpClientConfig::builder()
            .max_request_body(1024)
            .timeout(std::time::Duration::from_secs(10))
            .build();

        assert_eq!(config.max_request_body, 1024);
        assert_eq!(config.timeout, std::time::Duration::from_secs(10));
    }

    #[test]
    fn test_http_error_display() {
        let err = HttpError::HostNotAllowed { host: "evil.com".to_string() };
        assert!(err.to_string().contains("evil.com"));
        assert!(err.to_string().contains("not allowed"));
    }
}
