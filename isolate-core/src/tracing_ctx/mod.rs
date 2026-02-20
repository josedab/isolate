//! # Distributed Trace Context Propagation
//!
//! W3C Trace Context compliant distributed tracing for sandbox executions.
//! Provides trace context injection, extraction, span management, and
//! propagation across sandbox boundaries.
//!
//! Implements the [W3C Trace Context](https://www.w3.org/TR/trace-context/)
//! specification for `traceparent` and `tracestate` headers.
//!
//! ## Example
//!
//! ```rust
//! use isolate_core::tracing_ctx::{TraceContextPropagator, SpanContext, SpanBuilder};
//!
//! // Parse incoming W3C traceparent header
//! let propagator = TraceContextPropagator::new();
//! let ctx = propagator.extract_traceparent(
//!     "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
//! ).unwrap();
//! assert_eq!(ctx.trace_id(), "4bf92f3577b34da6a3ce929d0e0e4736");
//!
//! // Create a child span for sandbox execution
//! let child = SpanBuilder::new("sandbox.run")
//!     .with_parent(&ctx)
//!     .with_attribute("sandbox.id", "sb-123")
//!     .build();
//! ```

#![allow(missing_docs)]
mod context;
mod propagator;
mod span;

pub use context::{SpanContext, TraceFlags};
pub use propagator::TraceContextPropagator;
pub use span::{Span, SpanBuilder, SpanEvent, SpanKind, SpanRecorder, SpanStatus};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_full_propagation_flow() {
        let propagator = TraceContextPropagator::new();

        // Simulate incoming request with traceparent
        let parent_header = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
        let ctx = propagator.extract_traceparent(parent_header).unwrap();

        // Create child span for sandbox execution
        let child = SpanBuilder::new("sandbox.run")
            .with_parent(&ctx)
            .with_attribute("sandbox.id", "sb-123")
            .build();

        // Verify child inherits trace_id but gets new span_id
        assert_eq!(child.context.trace_id(), ctx.trace_id());
        assert_ne!(child.context.span_id(), ctx.span_id());

        // Re-inject for downstream propagation
        let header = propagator.inject_traceparent(&child.context);
        assert!(header.starts_with("00-4bf92f3577b34da6a3ce929d0e0e4736-"));
    }

    #[test]
    fn test_span_recorder_collects_spans() {
        let recorder = SpanRecorder::new(100);
        let span = SpanBuilder::new("test.op").build();
        recorder.record(span);

        let spans = recorder.drain();
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].name, "test.op");
    }
}
