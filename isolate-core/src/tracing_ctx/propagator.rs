use super::context::{SpanContext, TraceFlags};

/// W3C Trace Context propagator for `traceparent` and `tracestate` headers.
///
/// Parses and serializes trace context according to the W3C specification:
/// `{version}-{trace-id}-{parent-id}-{trace-flags}`
pub struct TraceContextPropagator;

impl TraceContextPropagator {
    pub fn new() -> Self {
        Self
    }

    /// Parse a W3C `traceparent` header into a `SpanContext`.
    ///
    /// Format: `{version:2}-{trace-id:32}-{parent-id:16}-{flags:2}`
    /// Example: `00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01`
    pub fn extract_traceparent(&self, header: &str) -> Option<SpanContext> {
        let parts: Vec<&str> = header.split('-').collect();
        if parts.len() != 4 {
            return None;
        }

        let version = parts[0];
        if version != "00" {
            return None;
        }

        let trace_id = parts[1];
        let span_id = parts[2];
        let flags_str = parts[3];

        // Validate lengths
        if trace_id.len() != 32 || span_id.len() != 16 || flags_str.len() != 2 {
            return None;
        }

        // Validate hex
        if !trace_id.chars().all(|c| c.is_ascii_hexdigit())
            || !span_id.chars().all(|c| c.is_ascii_hexdigit())
        {
            return None;
        }

        // All-zero trace-id or span-id is invalid
        if trace_id.chars().all(|c| c == '0') || span_id.chars().all(|c| c == '0') {
            return None;
        }

        let flags = u8::from_str_radix(flags_str, 16).ok()?;

        Some(SpanContext::from_w3c(
            trace_id.to_string(),
            span_id.to_string(),
            TraceFlags::new(flags),
        ))
    }

    /// Serialize a `SpanContext` into a W3C `traceparent` header value.
    pub fn inject_traceparent(&self, ctx: &SpanContext) -> String {
        format!("00-{}-{}-{}", ctx.trace_id(), ctx.span_id(), ctx.trace_flags())
    }

    /// Extract trace context from HTTP-style headers.
    pub fn extract_from_headers<'a>(
        &self,
        headers: impl Iterator<Item = (&'a str, &'a str)>,
    ) -> Option<SpanContext> {
        let mut traceparent = None;
        let mut tracestate_entries = Vec::new();

        for (key, value) in headers {
            match key.to_lowercase().as_str() {
                "traceparent" => traceparent = Some(value.to_string()),
                "tracestate" => tracestate_entries.push(value.to_string()),
                _ => {}
            }
        }

        let mut ctx = self.extract_traceparent(traceparent.as_deref()?)?;

        // Parse tracestate entries
        for entry in tracestate_entries {
            for kv in entry.split(',') {
                let kv = kv.trim();
                if let Some((k, v)) = kv.split_once('=') {
                    ctx = ctx.with_trace_state(k.trim(), v.trim());
                }
            }
        }

        Some(ctx)
    }

    /// Inject trace context into a header map.
    pub fn inject_into_headers(&self, ctx: &SpanContext) -> Vec<(String, String)> {
        let mut headers = vec![("traceparent".to_string(), self.inject_traceparent(ctx))];

        if !ctx.trace_state().is_empty() {
            let tracestate: String = ctx
                .trace_state()
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join(",");
            headers.push(("tracestate".to_string(), tracestate));
        }

        headers
    }
}

impl Default for TraceContextPropagator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_valid_traceparent() {
        let prop = TraceContextPropagator::new();
        let ctx = prop
            .extract_traceparent("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01")
            .unwrap();

        assert_eq!(ctx.trace_id(), "4bf92f3577b34da6a3ce929d0e0e4736");
        assert_eq!(ctx.span_id(), "00f067aa0ba902b7");
        assert!(ctx.is_sampled());
    }

    #[test]
    fn test_extract_not_sampled() {
        let prop = TraceContextPropagator::new();
        let ctx = prop
            .extract_traceparent("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-00")
            .unwrap();
        assert!(!ctx.is_sampled());
    }

    #[test]
    fn test_extract_invalid_version() {
        let prop = TraceContextPropagator::new();
        assert!(prop
            .extract_traceparent("01-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01")
            .is_none());
    }

    #[test]
    fn test_extract_all_zero_trace_id() {
        let prop = TraceContextPropagator::new();
        assert!(prop
            .extract_traceparent("00-00000000000000000000000000000000-00f067aa0ba902b7-01")
            .is_none());
    }

    #[test]
    fn test_extract_all_zero_span_id() {
        let prop = TraceContextPropagator::new();
        assert!(prop
            .extract_traceparent("00-4bf92f3577b34da6a3ce929d0e0e4736-0000000000000000-01")
            .is_none());
    }

    #[test]
    fn test_extract_wrong_format() {
        let prop = TraceContextPropagator::new();
        assert!(prop.extract_traceparent("not-valid").is_none());
        assert!(prop.extract_traceparent("").is_none());
        assert!(prop.extract_traceparent("00-short-id-01").is_none());
    }

    #[test]
    fn test_inject_traceparent() {
        let prop = TraceContextPropagator::new();
        let ctx = prop
            .extract_traceparent("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01")
            .unwrap();
        let header = prop.inject_traceparent(&ctx);
        assert_eq!(header, "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01");
    }

    #[test]
    fn test_roundtrip() {
        let prop = TraceContextPropagator::new();
        let original = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
        let ctx = prop.extract_traceparent(original).unwrap();
        let serialized = prop.inject_traceparent(&ctx);
        assert_eq!(serialized, original);
    }

    #[test]
    fn test_extract_from_headers() {
        let prop = TraceContextPropagator::new();
        let headers = vec![
            ("traceparent", "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"),
            ("tracestate", "vendor1=value1,vendor2=value2"),
        ];
        let ctx = prop.extract_from_headers(headers.into_iter()).unwrap();
        assert_eq!(ctx.trace_state().get("vendor1").unwrap(), "value1");
        assert_eq!(ctx.trace_state().get("vendor2").unwrap(), "value2");
    }

    #[test]
    fn test_inject_into_headers() {
        let prop = TraceContextPropagator::new();
        let ctx = prop
            .extract_traceparent("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01")
            .unwrap()
            .with_trace_state("isolate", "sandbox-1");

        let headers = prop.inject_into_headers(&ctx);
        assert!(headers.iter().any(|(k, _)| k == "traceparent"));
        assert!(headers.iter().any(|(k, _)| k == "tracestate"));
    }
}
