//! HTTP response types.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// An HTTP response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpResponse {
    /// HTTP status code.
    pub status: u16,
    /// Response headers.
    pub headers: HashMap<String, String>,
    /// Response body.
    pub body: HttpResponseBody,
    /// Time taken for the request.
    #[serde(skip)]
    pub duration: Duration,
    /// Final URL after redirects.
    pub final_url: String,
}

impl HttpResponse {
    /// Check if the response was successful (2xx status code).
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// Check if the response was a client error (4xx status code).
    pub fn is_client_error(&self) -> bool {
        (400..500).contains(&self.status)
    }

    /// Check if the response was a server error (5xx status code).
    pub fn is_server_error(&self) -> bool {
        (500..600).contains(&self.status)
    }

    /// Get the content type header.
    pub fn content_type(&self) -> Option<&str> {
        self.headers
            .get("content-type")
            .or_else(|| self.headers.get("Content-Type"))
            .map(|s| s.as_str())
    }

    /// Check if the response is JSON.
    pub fn is_json(&self) -> bool {
        self.content_type()
            .map(|ct| ct.starts_with("application/json"))
            .unwrap_or(false)
    }

    /// Get the body as bytes.
    pub fn bytes(&self) -> &[u8] {
        self.body.as_bytes()
    }

    /// Get the body as a string.
    pub fn text(&self) -> Result<&str, std::str::Utf8Error> {
        self.body.as_str()
    }

    /// Parse the body as JSON.
    pub fn json<T: for<'de> Deserialize<'de>>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_slice(self.body.as_bytes())
    }
}

/// HTTP response body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpResponseBody {
    /// The raw body bytes.
    data: Vec<u8>,
    /// Whether the body was truncated.
    truncated: bool,
}

impl HttpResponseBody {
    /// Create a new response body.
    pub fn new(data: Vec<u8>) -> Self {
        Self {
            data,
            truncated: false,
        }
    }

    /// Create a truncated response body.
    pub fn truncated(data: Vec<u8>) -> Self {
        Self {
            data,
            truncated: true,
        }
    }

    /// Get the body as bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }

    /// Get the body as a string.
    pub fn as_str(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(&self.data)
    }

    /// Check if the body was truncated.
    pub fn is_truncated(&self) -> bool {
        self.truncated
    }

    /// Get the length of the body.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Check if the body is empty.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Consume and return the underlying bytes.
    pub fn into_bytes(self) -> Vec<u8> {
        self.data
    }
}

impl From<Vec<u8>> for HttpResponseBody {
    fn from(data: Vec<u8>) -> Self {
        Self::new(data)
    }
}

impl From<&[u8]> for HttpResponseBody {
    fn from(data: &[u8]) -> Self {
        Self::new(data.to_vec())
    }
}

impl From<String> for HttpResponseBody {
    fn from(data: String) -> Self {
        Self::new(data.into_bytes())
    }
}

impl From<&str> for HttpResponseBody {
    fn from(data: &str) -> Self {
        Self::new(data.as_bytes().to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_response(status: u16, body: &str) -> HttpResponse {
        HttpResponse {
            status,
            headers: HashMap::new(),
            body: HttpResponseBody::new(body.as_bytes().to_vec()),
            duration: Duration::from_millis(100),
            final_url: "https://example.com".to_string(),
        }
    }

    #[test]
    fn test_response_status_checks() {
        assert!(mock_response(200, "").is_success());
        assert!(mock_response(201, "").is_success());
        assert!(!mock_response(404, "").is_success());

        assert!(mock_response(404, "").is_client_error());
        assert!(mock_response(400, "").is_client_error());
        assert!(!mock_response(500, "").is_client_error());

        assert!(mock_response(500, "").is_server_error());
        assert!(mock_response(503, "").is_server_error());
        assert!(!mock_response(400, "").is_server_error());
    }

    #[test]
    fn test_response_body_text() {
        let response = mock_response(200, "Hello, World!");
        assert_eq!(response.text().unwrap(), "Hello, World!");
    }

    #[test]
    fn test_response_body_json() {
        #[derive(Debug, PartialEq, Deserialize)]
        struct Data {
            name: String,
        }

        let response = HttpResponse {
            status: 200,
            headers: HashMap::new(),
            body: HttpResponseBody::new(br#"{"name":"test"}"#.to_vec()),
            duration: Duration::from_millis(100),
            final_url: "https://example.com".to_string(),
        };

        let data: Data = response.json().unwrap();
        assert_eq!(data.name, "test");
    }

    #[test]
    fn test_response_content_type() {
        let mut response = mock_response(200, "");
        assert!(response.content_type().is_none());

        response
            .headers
            .insert("content-type".to_string(), "application/json".to_string());
        assert_eq!(response.content_type(), Some("application/json"));
        assert!(response.is_json());
    }

    #[test]
    fn test_response_body_truncated() {
        let body = HttpResponseBody::truncated(b"partial data".to_vec());
        assert!(body.is_truncated());
        assert_eq!(body.len(), 12);
    }
}
