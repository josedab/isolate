//! HTTP client implementation with capability enforcement.

use super::{HttpClientConfig, HttpError, HttpRequest, HttpResponse, HttpResponseBody};
use crate::capability::{Capability, CapabilitySet, NetworkCapability};
use crate::error::Result;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, instrument, warn};
use url::Url;

/// HTTP client with capability-based access control.
///
/// This client enforces that only requests to explicitly allowed hosts
/// can be made, providing security for sandboxed code execution.
pub struct HttpClient {
    /// The underlying reqwest client.
    client: reqwest::Client,
    /// Configuration.
    config: HttpClientConfig,
    /// Allowed capabilities.
    capabilities: Arc<CapabilitySet>,
}

impl HttpClient {
    /// Create a new HTTP client with the given capabilities.
    pub fn new(capabilities: impl Into<CapabilitySet>) -> Result<Self> {
        Self::with_config(capabilities, HttpClientConfig::default())
    }

    /// Create a new HTTP client with custom configuration.
    pub fn with_config(
        capabilities: impl Into<CapabilitySet>,
        config: HttpClientConfig,
    ) -> Result<Self> {
        let mut client_builder = reqwest::Client::builder()
            .timeout(config.timeout)
            .redirect(reqwest::redirect::Policy::limited(config.max_redirects))
            .user_agent(&config.user_agent);

        if config.allow_insecure_tls {
            client_builder = client_builder.danger_accept_invalid_certs(true);
        }

        let client = client_builder
            .build()
            .map_err(|e| crate::Error::Http(format!("Failed to create HTTP client: {}", e)))?;

        Ok(Self { client, config, capabilities: Arc::new(capabilities.into()) })
    }

    /// Execute an HTTP request.
    ///
    /// This method checks that the request is allowed by the capabilities
    /// before making the actual HTTP call.
    #[instrument(skip(self, request), fields(method = %request.method, url = %request.url))]
    pub async fn execute(&self, request: HttpRequest) -> Result<HttpResponse> {
        // Parse and validate URL
        let url = Url::parse(&request.url)
            .map_err(|e| HttpError::InvalidUrl(format!("{}: {}", request.url, e)))?;

        // Check capability
        let host =
            url.host_str().ok_or_else(|| HttpError::InvalidUrl("URL has no host".to_string()))?;

        self.check_host_allowed(host)?;

        // Check request body size
        if let Some(ref body) = request.body {
            if body.len() > self.config.max_request_body {
                return Err(
                    HttpError::RequestBodyTooLarge { max: self.config.max_request_body }.into()
                );
            }
        }

        debug!("Making HTTP {} request to {}", request.method, host);

        // Build the request
        let start = Instant::now();
        let mut req_builder = self.client.request(request.method.into(), url.as_str());

        // Add headers
        for (name, value) in &request.headers {
            req_builder = req_builder.header(name, value);
        }

        // Add body if present
        if let Some(body) = request.body {
            req_builder = req_builder.body(body);
        }

        // Set timeout if specified
        if let Some(timeout) = request.timeout {
            req_builder = req_builder.timeout(timeout);
        }

        // Execute the request
        let response = req_builder.send().await.map_err(|e| {
            if e.is_timeout() {
                HttpError::Timeout(self.config.timeout)
            } else if e.is_connect() {
                HttpError::Connection(e.to_string())
            } else if e.is_redirect() {
                HttpError::TooManyRedirects { max: self.config.max_redirects }
            } else {
                HttpError::Http(e.to_string())
            }
        })?;

        let duration = start.elapsed();
        let status = response.status().as_u16();
        let final_url = response.url().to_string();

        // Extract headers
        let mut headers = std::collections::HashMap::new();
        for (name, value) in response.headers() {
            if let Ok(value_str) = value.to_str() {
                headers.insert(name.to_string(), value_str.to_string());
            }
        }

        // Read body with size limit
        let body_bytes = response
            .bytes()
            .await
            .map_err(|e| HttpError::Http(format!("Failed to read response body: {}", e)))?;

        let body = if body_bytes.len() > self.config.max_response_body {
            warn!(
                "Response body truncated from {} to {} bytes",
                body_bytes.len(),
                self.config.max_response_body
            );
            HttpResponseBody::truncated(body_bytes[..self.config.max_response_body].to_vec())
        } else {
            HttpResponseBody::new(body_bytes.to_vec())
        };

        debug!("HTTP response: {} {} in {:?} ({} bytes)", status, final_url, duration, body.len());

        Ok(HttpResponse { status, headers, body, duration, final_url })
    }

    /// Check if the host is allowed by the capabilities.
    fn check_host_allowed(&self, host: &str) -> Result<()> {
        for cap in self.capabilities.iter() {
            if let Capability::Network(NetworkCapability::HttpClient(hosts)) = cap {
                for pattern in hosts {
                    if Self::host_matches_pattern(host, pattern) {
                        return Ok(());
                    }
                }
            }
        }

        Err(HttpError::HostNotAllowed { host: host.to_string() }.into())
    }

    /// Check if a host matches a pattern.
    fn host_matches_pattern(host: &str, pattern: &str) -> bool {
        if pattern.starts_with("*.") {
            // Wildcard subdomain pattern (e.g., "*.example.com")
            let suffix = &pattern[1..]; // ".example.com"
            host.ends_with(suffix) || host == &pattern[2..]
        } else if pattern == "*" {
            // Match all hosts
            true
        } else {
            // Exact match
            host == pattern
        }
    }

    /// Execute a GET request.
    pub async fn get(&self, url: impl Into<String>) -> Result<HttpResponse> {
        self.execute(HttpRequest::get(url)).await
    }

    /// Execute a POST request with JSON body.
    pub async fn post_json<T: serde::Serialize>(
        &self,
        url: impl Into<String>,
        body: &T,
    ) -> Result<HttpResponse> {
        let request = HttpRequest::post(url)
            .json(body)
            .map_err(|e| crate::Error::Http(format!("Failed to serialize JSON: {}", e)))?;
        self.execute(request).await
    }
}

impl std::fmt::Debug for HttpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpClient")
            .field("config", &self.config)
            .field("capabilities", &self.capabilities)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_host_matches_pattern_exact() {
        assert!(HttpClient::host_matches_pattern("api.example.com", "api.example.com"));
        assert!(!HttpClient::host_matches_pattern("api.example.com", "other.example.com"));
    }

    #[test]
    fn test_host_matches_pattern_wildcard() {
        assert!(HttpClient::host_matches_pattern("api.example.com", "*.example.com"));
        assert!(HttpClient::host_matches_pattern("sub.api.example.com", "*.example.com"));
        assert!(HttpClient::host_matches_pattern("example.com", "*.example.com"));
        assert!(!HttpClient::host_matches_pattern("example.org", "*.example.com"));
    }

    #[test]
    fn test_host_matches_pattern_star() {
        assert!(HttpClient::host_matches_pattern("any.host.com", "*"));
        assert!(HttpClient::host_matches_pattern("localhost", "*"));
    }

    #[test]
    fn test_check_host_allowed() {
        let caps = CapabilitySet::from_iter(vec![
            Capability::http_client(vec!["api.example.com"]),
            Capability::http_client(vec!["*.trusted.org"]),
        ]);
        let client = HttpClient::new(caps).unwrap();

        assert!(client.check_host_allowed("api.example.com").is_ok());
        assert!(client.check_host_allowed("sub.trusted.org").is_ok());
        assert!(client.check_host_allowed("evil.com").is_err());
    }

    #[test]
    fn test_http_client_creation() {
        let caps = CapabilitySet::from_iter(vec![Capability::http_client(vec!["example.com"])]);
        let client = HttpClient::new(caps);
        assert!(client.is_ok());
    }

    #[test]
    fn test_http_client_with_config() {
        let caps = CapabilitySet::from_iter(vec![Capability::http_client(vec!["example.com"])]);
        let config = HttpClientConfig::builder()
            .timeout(std::time::Duration::from_secs(5))
            .max_redirects(5)
            .build();

        let client = HttpClient::with_config(caps, config);
        assert!(client.is_ok());
    }
}
