//! HTTP request types.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// HTTP methods.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    /// GET request.
    Get,
    /// POST request.
    Post,
    /// PUT request.
    Put,
    /// DELETE request.
    Delete,
    /// PATCH request.
    Patch,
    /// HEAD request.
    Head,
    /// OPTIONS request.
    Options,
}

impl std::fmt::Display for HttpMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Get => write!(f, "GET"),
            Self::Post => write!(f, "POST"),
            Self::Put => write!(f, "PUT"),
            Self::Delete => write!(f, "DELETE"),
            Self::Patch => write!(f, "PATCH"),
            Self::Head => write!(f, "HEAD"),
            Self::Options => write!(f, "OPTIONS"),
        }
    }
}

impl From<HttpMethod> for reqwest::Method {
    fn from(method: HttpMethod) -> Self {
        match method {
            HttpMethod::Get => reqwest::Method::GET,
            HttpMethod::Post => reqwest::Method::POST,
            HttpMethod::Put => reqwest::Method::PUT,
            HttpMethod::Delete => reqwest::Method::DELETE,
            HttpMethod::Patch => reqwest::Method::PATCH,
            HttpMethod::Head => reqwest::Method::HEAD,
            HttpMethod::Options => reqwest::Method::OPTIONS,
        }
    }
}

/// An HTTP request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpRequest {
    /// The HTTP method.
    pub method: HttpMethod,
    /// The URL to request.
    pub url: String,
    /// Request headers.
    pub headers: HashMap<String, String>,
    /// Request body (if any).
    #[serde(default)]
    pub body: Option<Vec<u8>>,
    /// Request timeout (overrides client default).
    #[serde(skip)]
    pub timeout: Option<Duration>,
}

impl HttpRequest {
    /// Create a new GET request.
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            method: HttpMethod::Get,
            url: url.into(),
            headers: HashMap::new(),
            body: None,
            timeout: None,
        }
    }

    /// Create a new POST request.
    pub fn post(url: impl Into<String>) -> Self {
        Self {
            method: HttpMethod::Post,
            url: url.into(),
            headers: HashMap::new(),
            body: None,
            timeout: None,
        }
    }

    /// Create a new PUT request.
    pub fn put(url: impl Into<String>) -> Self {
        Self {
            method: HttpMethod::Put,
            url: url.into(),
            headers: HashMap::new(),
            body: None,
            timeout: None,
        }
    }

    /// Create a new DELETE request.
    pub fn delete(url: impl Into<String>) -> Self {
        Self {
            method: HttpMethod::Delete,
            url: url.into(),
            headers: HashMap::new(),
            body: None,
            timeout: None,
        }
    }

    /// Create a new PATCH request.
    pub fn patch(url: impl Into<String>) -> Self {
        Self {
            method: HttpMethod::Patch,
            url: url.into(),
            headers: HashMap::new(),
            body: None,
            timeout: None,
        }
    }

    /// Create a new request builder.
    pub fn builder() -> HttpRequestBuilder {
        HttpRequestBuilder::default()
    }

    /// Add a header to the request.
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    /// Set the request body.
    pub fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// Set the request body as JSON.
    pub fn json<T: Serialize>(mut self, value: &T) -> Result<Self, serde_json::Error> {
        let json = serde_json::to_vec(value)?;
        self.headers
            .insert("Content-Type".to_string(), "application/json".to_string());
        self.body = Some(json);
        Ok(self)
    }

    /// Set the request timeout.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }
}

/// Builder for HTTP requests.
#[derive(Debug, Default)]
pub struct HttpRequestBuilder {
    method: Option<HttpMethod>,
    url: Option<String>,
    headers: HashMap<String, String>,
    body: Option<Vec<u8>>,
    timeout: Option<Duration>,
}

impl HttpRequestBuilder {
    /// Set the HTTP method.
    pub fn method(mut self, method: HttpMethod) -> Self {
        self.method = Some(method);
        self
    }

    /// Set the URL.
    pub fn url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }

    /// Add a header.
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    /// Set the body.
    pub fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// Set the body as JSON.
    pub fn json<T: Serialize>(mut self, value: &T) -> Result<Self, serde_json::Error> {
        let json = serde_json::to_vec(value)?;
        self.headers
            .insert("Content-Type".to_string(), "application/json".to_string());
        self.body = Some(json);
        Ok(self)
    }

    /// Set the timeout.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Build the request.
    pub fn build(self) -> Result<HttpRequest, &'static str> {
        let method = self.method.ok_or("method is required")?;
        let url = self.url.ok_or("url is required")?;

        Ok(HttpRequest {
            method,
            url,
            headers: self.headers,
            body: self.body,
            timeout: self.timeout,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http_method_display() {
        assert_eq!(HttpMethod::Get.to_string(), "GET");
        assert_eq!(HttpMethod::Post.to_string(), "POST");
        assert_eq!(HttpMethod::Delete.to_string(), "DELETE");
    }

    #[test]
    fn test_http_request_get() {
        let req = HttpRequest::get("https://example.com");
        assert_eq!(req.method, HttpMethod::Get);
        assert_eq!(req.url, "https://example.com");
        assert!(req.body.is_none());
    }

    #[test]
    fn test_http_request_with_headers() {
        let req = HttpRequest::get("https://example.com")
            .header("Authorization", "Bearer token")
            .header("Accept", "application/json");

        assert_eq!(
            req.headers.get("Authorization"),
            Some(&"Bearer token".to_string())
        );
        assert_eq!(
            req.headers.get("Accept"),
            Some(&"application/json".to_string())
        );
    }

    #[test]
    fn test_http_request_with_json_body() {
        #[derive(Serialize)]
        struct Data {
            name: String,
        }

        let req = HttpRequest::post("https://example.com")
            .json(&Data {
                name: "test".to_string(),
            })
            .unwrap();

        assert!(req.body.is_some());
        assert_eq!(
            req.headers.get("Content-Type"),
            Some(&"application/json".to_string())
        );
    }

    #[test]
    fn test_http_request_builder() {
        let req = HttpRequest::builder()
            .method(HttpMethod::Post)
            .url("https://example.com")
            .header("X-Custom", "value")
            .body(b"hello".to_vec())
            .build()
            .unwrap();

        assert_eq!(req.method, HttpMethod::Post);
        assert_eq!(req.url, "https://example.com");
        assert_eq!(req.body, Some(b"hello".to_vec()));
    }

    #[test]
    fn test_http_request_builder_missing_method() {
        let result = HttpRequest::builder().url("https://example.com").build();

        assert!(result.is_err());
    }
}
