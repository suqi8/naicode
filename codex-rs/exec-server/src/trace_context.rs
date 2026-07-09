use reqwest::header::HeaderMap;
use reqwest::header::HeaderValue;

/// Sets the current OpenTelemetry context as `span`'s parent without creating
/// a structural `tracing` parent relationship.
///
/// Callers must construct `span` with `parent: None`. This lets work that can
/// outlive the current span remain correlated without keeping the current span
/// open until the detached work finishes.
pub(crate) fn set_current_trace_context_as_parent(span: &tracing::Span) {
    let Some(trace) = codex_otel::current_span_w3c_trace_context() else {
        return;
    };
    let _ = codex_otel::set_parent_from_w3c_trace_context(span, &trace);
}

pub(crate) fn current_trace_context_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    let Some(trace) = codex_otel::current_span_w3c_trace_context() else {
        return headers;
    };
    if let Some(traceparent) = trace.traceparent
        && let Ok(value) = HeaderValue::try_from(traceparent)
    {
        headers.insert("traceparent", value);
    }
    if let Some(tracestate) = trace.tracestate
        && let Ok(value) = HeaderValue::try_from(tracestate)
    {
        headers.insert("tracestate", value);
    }
    headers
}

#[cfg(test)]
#[path = "trace_context_tests.rs"]
mod tests;
