// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! JSON lines: `ts`, `level`, `target`, `thread`, `span`, `msg`, then the
//! event's own fields in order (11). Lines are redacted before any output.

use std::fmt::{self, Write as _};
use std::time::SystemTime;

use serde_json::Value;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::Layer;

use crate::sensitive::{redact, sensitive_logging};

/// Where finished lines go. `line` has no trailing newline.
pub trait LineSink: Send + Sync + 'static {
    fn write_line(&self, line: String);
}

pub struct JsonLayer<K> {
    sink: K,
    now: fn() -> SystemTime,
}

impl<K: LineSink> JsonLayer<K> {
    pub fn new(sink: K) -> Self {
        JsonLayer {
            sink,
            now: SystemTime::now,
        }
    }

    #[cfg(test)]
    pub fn with_clock(sink: K, now: fn() -> SystemTime) -> Self {
        JsonLayer { sink, now }
    }
}

/// A span's name and fields as shown in `span`, e.g. `board{id=main}`.
struct SpanText(String);

impl<S, K> Layer<S> for JsonLayer<K>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    K: LineSink,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        let mut fields = SpanFields(String::new());
        attrs.record(&mut fields);
        span.extensions_mut().insert(SpanText(fields.0));
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        let mut ext = span.extensions_mut();
        if let Some(text) = ext.get_mut::<SpanText>() {
            let mut fields = SpanFields(std::mem::take(&mut text.0));
            values.record(&mut fields);
            text.0 = fields.0;
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let meta = event.metadata();
        let mut spans = String::new();
        if let Some(scope) = ctx.event_scope(event) {
            for span in scope.from_root() {
                if !spans.is_empty() {
                    spans.push(':');
                }
                spans.push_str(span.name());
                let ext = span.extensions();
                if let Some(SpanText(f)) = ext.get::<SpanText>() {
                    if !f.is_empty() {
                        let _ = write!(spans, "{{{f}}}");
                    }
                }
            }
        }
        let mut fields = EventFields::default();
        event.record(&mut fields);

        let mut line = String::with_capacity(256);
        line.push('{');
        push_pair(&mut line, "ts", &crate::time::rfc3339_micros((self.now)()));
        line.push(',');
        push_pair(&mut line, "level", meta.level().as_str());
        line.push(',');
        push_pair(&mut line, "target", meta.target());
        line.push(',');
        let thread = std::thread::current();
        push_pair(&mut line, "thread", thread.name().unwrap_or("unnamed"));
        if !spans.is_empty() {
            line.push(',');
            push_pair(&mut line, "span", &spans);
        }
        line.push(',');
        push_pair(&mut line, "msg", fields.message.as_deref().unwrap_or(""));
        for (name, value) in fields.fields {
            line.push(',');
            push_json(&mut line, &Value::from(name));
            line.push(':');
            push_json(&mut line, &value);
        }
        line.push('}');
        let line = match redact(&line, sensitive_logging()) {
            std::borrow::Cow::Borrowed(_) => line,
            std::borrow::Cow::Owned(l) => l,
        };
        self.sink.write_line(line);
    }
}

fn push_pair(out: &mut String, key: &str, value: &str) {
    push_json(out, &Value::from(key));
    out.push(':');
    push_json(out, &Value::from(value));
}

fn push_json(out: &mut String, v: &Value) {
    // Serialising a `Value` into a `String` cannot fail.
    if let Ok(s) = serde_json::to_string(v) {
        out.push_str(&s);
    }
}

#[derive(Default)]
struct EventFields {
    message: Option<String>,
    fields: Vec<(&'static str, Value)>,
}

impl EventFields {
    fn put(&mut self, field: &Field, value: Value) {
        if field.name() == "message" {
            self.message = Some(match value {
                Value::String(s) => s,
                other => other.to_string(),
            });
        } else if !field.name().starts_with("log.") {
            self.fields.push((field.name(), value));
        }
    }
}

impl Visit for EventFields {
    fn record_f64(&mut self, field: &Field, value: f64) {
        self.put(field, Value::from(value));
    }
    fn record_i64(&mut self, field: &Field, value: i64) {
        self.put(field, Value::from(value));
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        self.put(field, Value::from(value));
    }
    fn record_bool(&mut self, field: &Field, value: bool) {
        self.put(field, Value::from(value));
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        self.put(field, Value::from(value));
    }
    fn record_error(&mut self, field: &Field, value: &(dyn std::error::Error + 'static)) {
        // The full chain (11).
        self.put(field, Value::from(chain(value)));
    }
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.put(field, Value::from(format!("{value:?}")));
    }
}

fn chain(e: &(dyn std::error::Error + 'static)) -> String {
    let mut s = e.to_string();
    let mut source = e.source();
    while let Some(inner) = source {
        let _ = write!(s, ": {inner}");
        source = inner.source();
    }
    s
}

/// `k=v` pairs separated by spaces, values in Display form.
struct SpanFields(String);

impl SpanFields {
    fn push(&mut self, field: &Field, value: fmt::Arguments<'_>) {
        if !self.0.is_empty() {
            self.0.push(' ');
        }
        let _ = write!(self.0, "{}={}", field.name(), value);
    }
}

impl Visit for SpanFields {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.push(field, format_args!("{value}"));
    }
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.push(field, format_args!("{value:?}"));
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::sensitive::{Sensitive, SensitiveKey};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, UNIX_EPOCH};
    use tracing_subscriber::layer::SubscriberExt;

    #[derive(Clone, Default)]
    pub(crate) struct Lines(pub Arc<Mutex<Vec<String>>>);

    impl LineSink for Lines {
        fn write_line(&self, line: String) {
            self.0.lock().unwrap().push(line);
        }
    }

    impl Lines {
        pub(crate) fn take(&self) -> Vec<String> {
            std::mem::take(&mut *self.0.lock().unwrap())
        }
    }

    fn spec_time() -> SystemTime {
        UNIX_EPOCH + Duration::from_micros(1_791_755_403_214_512)
    }

    fn capture(f: impl FnOnce()) -> Vec<String> {
        let lines = Lines::default();
        let sub =
            tracing_subscriber::registry().with(JsonLayer::with_clock(lines.clone(), spec_time));
        tracing::subscriber::with_default(sub, f);
        lines.take()
    }

    #[test]
    fn matches_the_spec_example() {
        let lines = std::thread::Builder::new()
            .name("engine".into())
            .spawn(|| {
                capture(|| {
                    let span = tracing::info_span!("board", id = "main-keyboard");
                    let _g = span.enter();
                    tracing::debug!(
                        target: "uca::engine::dwell",
                        item = "main-keyboard/letters/key-q",
                        elapsed_ms = 801,
                        "dwell activated"
                    );
                })
            })
            .unwrap()
            .join()
            .unwrap();
        assert_eq!(
            lines,
            [
                r#"{"ts":"2026-10-11T21:50:03.214512Z","level":"DEBUG","target":"uca::engine::dwell","thread":"engine","span":"board{id=main-keyboard}","msg":"dwell activated","item":"main-keyboard/letters/key-q","elapsed_ms":801}"#
            ]
        );
    }

    #[test]
    fn nested_spans_join_with_colons() {
        let lines = capture(|| {
            let outer = tracing::info_span!("plugin", id = "x");
            let _o = outer.enter();
            let inner = tracing::info_span!("locate");
            let _i = inner.enter();
            tracing::info!(target: "uca::app", "hi");
        });
        assert!(
            lines[0].contains(r#""span":"plugin{id=x}:locate""#),
            "{}",
            lines[0]
        );
    }

    #[test]
    fn sensitive_values_are_redacted_and_escaped() {
        let lines = capture(|| {
            tracing::debug!(
                target: "uca::actions",
                text = %Sensitive("se\"cret"),
                key = ?SensitiveKey("KeyQ"),
                "typed {}", Sensitive("secret")
            );
        });
        let line = &lines[0];
        assert!(!line.contains("secret") && !line.contains("cret"), "{line}");
        assert!(!line.contains("KeyQ"), "{line}");
        assert!(
            line.contains(r#""msg":"typed <redacted:6 chars>""#),
            "{line}"
        );
        assert!(line.contains(r#""text":"<redacted:7 chars>""#), "{line}");
        assert!(line.contains(r#""key":"<redacted key>""#), "{line}");
        let v: Value = serde_json::from_str(line).unwrap();
        assert_eq!(v["level"], "DEBUG");
    }

    #[test]
    fn errors_log_their_whole_chain() {
        #[derive(Debug)]
        struct Outer(std::io::Error);
        impl fmt::Display for Outer {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("cannot save")
            }
        }
        impl std::error::Error for Outer {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                Some(&self.0)
            }
        }
        let e = Outer(std::io::Error::other("disk full"));
        let lines = capture(|| {
            tracing::error!(target: "uca::config", error = &e as &dyn std::error::Error, "save failed");
        });
        assert!(
            lines[0].contains(r#""error":"cannot save: disk full""#),
            "{}",
            lines[0]
        );
    }
}
