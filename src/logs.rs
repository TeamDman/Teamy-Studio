use chrono::{DateTime, Local};
use std::collections::VecDeque;
use std::fmt;
use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread::{Builder, JoinHandle};
use tracing::field::{Field, Visit};
use tracing::{Event, Id, Level, Subscriber, span};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;

use crate::timeline::{
    TimelineDataset, TimelineFieldInputValue, TimelineInstantNs, TimelineItemInput,
};

const MAX_LOG_RECORDS: usize = 4_096;
const MAX_LOG_SPAN_RECORDS: usize = 4_096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogRecordLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogRecordLevel {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Trace => "TRACE",
            Self::Debug => "DEBUG",
            Self::Info => "INFO",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
        }
    }

    fn from_tracing(level: Level) -> Self {
        if level == Level::TRACE {
            Self::Trace
        } else if level == Level::DEBUG {
            Self::Debug
        } else if level == Level::INFO {
            Self::Info
        } else if level == Level::WARN {
            Self::Warn
        } else {
            Self::Error
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogRecordSnapshot {
    pub id: u64,
    pub timestamp: DateTime<Local>,
    pub level: LogRecordLevel,
    pub thread_name: String,
    pub thread_key: String,
    pub target: String,
    pub message: String,
    pub source_hwnd: Option<isize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogSpanSnapshot {
    pub id: u64,
    pub start_timestamp: DateTime<Local>,
    pub end_timestamp: DateTime<Local>,
    pub thread_name: String,
    pub thread_key: String,
    pub target: String,
    pub name: String,
    pub fields: Vec<String>,
    pub source_hwnd: Option<isize>,
}

impl LogRecordSnapshot {
    #[must_use]
    pub fn time_text(&self) -> String {
        self.timestamp.format("%H:%M:%S%.3f").to_string()
    }
}

#[derive(Debug)]
struct LogsState {
    next_id: AtomicU64,
    next_span_id: AtomicU64,
    records: Mutex<VecDeque<LogRecordSnapshot>>,
    span_records: Mutex<VecDeque<LogSpanSnapshot>>,
}

impl Default for LogsState {
    fn default() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            next_span_id: AtomicU64::new(1),
            records: Mutex::new(VecDeque::with_capacity(MAX_LOG_RECORDS)),
            span_records: Mutex::new(VecDeque::with_capacity(MAX_LOG_SPAN_RECORDS)),
        }
    }
}

static LOGS_STATE: OnceLock<LogsState> = OnceLock::new();

fn logs_state() -> &'static LogsState {
    LOGS_STATE.get_or_init(LogsState::default)
}

#[derive(Clone, Copy, Debug, Default)]
pub struct LogCollectorLayer;

pub trait ThreadBuilderSpanExt {
    /// Spawn a thread that enters the current tracing span before running `f`.
    ///
    /// # Errors
    ///
    /// Returns the underlying thread spawn error when the operating system cannot create the
    /// requested thread.
    fn spawn_with_current_span<F, T>(self, f: F) -> io::Result<JoinHandle<T>>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static;
}

impl ThreadBuilderSpanExt for Builder {
    // observability[impl logs.span-context]
    fn spawn_with_current_span<F, T>(self, f: F) -> io::Result<JoinHandle<T>>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        let dispatch = tracing::dispatcher::get_default(Clone::clone);
        let span = tracing::Span::current();
        self.spawn(move || {
            tracing::dispatcher::with_default(&dispatch, || {
                let _span = span.enter();
                f()
            })
        })
    }
}

#[derive(Clone, Debug)]
struct LogSpanFields {
    start_timestamp: DateTime<Local>,
    thread_name: String,
    thread_key: String,
    target: String,
    name: String,
    fields: Vec<String>,
    source_hwnd: Option<isize>,
}

impl<S> Layer<S> for LogCollectorLayer
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    // observability[impl logs.span-context]
    fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let mut visitor = LogFieldVisitor::default();
        attrs.record(&mut visitor);
        let Some(span) = ctx.span(id) else {
            return;
        };
        span.extensions_mut().replace(LogSpanFields {
            start_timestamp: Local::now(),
            thread_name: current_thread_name(),
            thread_key: current_thread_key(),
            target: attrs.metadata().target().to_owned(),
            name: attrs.metadata().name().to_owned(),
            fields: visitor.fields,
            source_hwnd: visitor.source_hwnd,
        });
    }

    // timeline[impl playground.live-tracing-spans]
    fn on_close(&self, id: Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(&id) else {
            return;
        };
        let Some(fields) = span.extensions().get::<LogSpanFields>().cloned() else {
            return;
        };
        push_log_span_record(fields, Local::now());
    }

    fn on_enter(&self, id: &Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else {
            return;
        };
        let mut extensions = span.extensions_mut();
        if let Some(fields) = extensions.get_mut::<LogSpanFields>() {
            fields.thread_name = current_thread_name();
            fields.thread_key = current_thread_key();
        }
    }

    fn on_record(&self, id: &Id, values: &span::Record<'_>, ctx: Context<'_, S>) {
        let mut visitor = LogFieldVisitor::default();
        values.record(&mut visitor);
        let Some(span) = ctx.span(id) else {
            return;
        };
        let mut extensions = span.extensions_mut();
        if let Some(fields) = extensions.get_mut::<LogSpanFields>() {
            if let Some(source_hwnd) = visitor.source_hwnd {
                fields.source_hwnd = Some(source_hwnd);
            }
            fields.fields.extend(visitor.fields);
        } else if let Some(source_hwnd) = visitor.source_hwnd {
            extensions.replace(LogSpanFields {
                start_timestamp: Local::now(),
                thread_name: current_thread_name(),
                thread_key: current_thread_key(),
                target: String::new(),
                name: "span".to_owned(),
                fields: visitor.fields,
                source_hwnd: Some(source_hwnd),
            });
        }
    }

    // observability[impl logs.capture]
    // observability[impl logs.span-context]
    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let mut visitor = LogFieldVisitor::default();
        event.record(&mut visitor);
        let metadata = event.metadata();
        let source_hwnd = visitor
            .source_hwnd
            .or_else(|| source_hwnd_from_event_scope(event, &ctx));
        let target = visitor
            .target
            .clone()
            .unwrap_or_else(|| metadata.target().to_owned());
        push_log_record(
            LogRecordLevel::from_tracing(*metadata.level()),
            &target,
            visitor.message_text(),
            source_hwnd,
        );
    }
}

fn source_hwnd_from_event_scope<S>(event: &Event<'_>, ctx: &Context<'_, S>) -> Option<isize>
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    ctx.event_scope(event)?.find_map(|span| {
        span.extensions()
            .get::<LogSpanFields>()
            .and_then(|fields| fields.source_hwnd)
    })
}

#[derive(Debug, Default)]
struct LogFieldVisitor {
    message: Option<String>,
    source_hwnd: Option<isize>,
    target: Option<String>,
    fields: Vec<String>,
}

impl LogFieldVisitor {
    fn record_value(&mut self, field: &Field, value: String) {
        self.record_named_value(field.name(), value);
    }

    fn record_named_value(&mut self, field_name: &str, value: String) {
        if field_name == "message" {
            self.message = Some(value);
        } else if field_name == "source_hwnd" {
            self.source_hwnd = value.parse().ok();
        } else if field_name == "log.target" {
            self.target = Some(value);
        } else {
            self.fields.push(format!("{field_name}={value}"));
        }
    }

    fn message_text(self) -> String {
        let Some(message) = self.message else {
            return self.fields.join(" ");
        };
        if self.fields.is_empty() {
            message
        } else {
            format!("{message} {}", self.fields.join(" "))
        }
    }
}

impl Visit for LogFieldVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.record_value(field, format!("{value:?}"));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.record_value(field, value.to_owned());
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.record_value(field, value.to_string());
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        if field.name() == "source_hwnd" {
            self.source_hwnd = isize::try_from(value).ok();
            return;
        }
        self.record_value(field, value.to_string());
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        if field.name() == "source_hwnd" {
            self.source_hwnd = isize::try_from(value).ok();
            return;
        }
        self.record_value(field, value.to_string());
    }
}

fn push_log_record(
    level: LogRecordLevel,
    target: &str,
    message: String,
    source_hwnd: Option<isize>,
) -> u64 {
    let state = logs_state();
    let id = state.next_id.fetch_add(1, Ordering::AcqRel);
    let mut records = state
        .records
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if records.len() == MAX_LOG_RECORDS {
        let _ = records.pop_front();
    }
    records.push_back(LogRecordSnapshot {
        id,
        timestamp: Local::now(),
        level,
        thread_name: current_thread_name(),
        thread_key: current_thread_key(),
        target: target.to_owned(),
        message,
        source_hwnd,
    });
    id
}

fn push_log_span_record(fields: LogSpanFields, end_timestamp: DateTime<Local>) -> u64 {
    let state = logs_state();
    let id = state.next_span_id.fetch_add(1, Ordering::AcqRel);
    let mut span_records = state
        .span_records
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if span_records.len() == MAX_LOG_SPAN_RECORDS {
        let _ = span_records.pop_front();
    }
    span_records.push_back(LogSpanSnapshot {
        id,
        start_timestamp: fields.start_timestamp,
        end_timestamp: end_timestamp.max(fields.start_timestamp),
        thread_name: fields.thread_name,
        thread_key: fields.thread_key,
        target: fields.target,
        name: fields.name,
        fields: fields.fields,
        source_hwnd: fields.source_hwnd,
    });
    id
}

fn current_thread_name() -> String {
    std::thread::current()
        .name()
        .unwrap_or("unnamed thread")
        .to_owned()
}

fn current_thread_key() -> String {
    let thread = std::thread::current();
    let name = thread.name().unwrap_or("unnamed thread");
    format!("{name} {:?}", thread.id())
}

#[must_use]
pub fn log_snapshots() -> Vec<LogRecordSnapshot> {
    logs_state()
        .records
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .cloned()
        .collect()
}

#[must_use]
pub fn log_span_snapshots() -> Vec<LogSpanSnapshot> {
    logs_state()
        .span_records
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .cloned()
        .collect()
}

#[must_use]
// timeline[impl playground.live-tracing-events]
pub fn tracing_event_timeline_dataset() -> (TimelineDataset, i64) {
    let records = log_snapshots();
    let span_records = log_span_snapshots();
    let mut dataset = TimelineDataset::new();
    let Some(first_timestamp) = records
        .iter()
        .map(|record| record.timestamp)
        .chain(span_records.iter().map(|record| record.start_timestamp))
        .min()
    else {
        return (dataset, 1);
    };
    let mut latest_at_ns = 1_i64;

    for record in span_records {
        let start_ns = record
            .start_timestamp
            .signed_duration_since(first_timestamp)
            .num_nanoseconds()
            .unwrap_or(latest_at_ns)
            .max(0);
        let end_ns = record
            .end_timestamp
            .signed_duration_since(first_timestamp)
            .num_nanoseconds()
            .unwrap_or(start_ns)
            .max(start_ns);
        latest_at_ns = latest_at_ns.max(end_ns);
        // timeline[impl playground.live-tracing-thread-identity]
        let mut input = TimelineItemInput::new(record.name.clone())
            .with_source_key(record.target.clone())
            .with_group_key(record.thread_key.clone())
            .with_field("span_id", TimelineFieldInputValue::U64(record.id))
            .with_field(
                "thread",
                TimelineFieldInputValue::String(record.thread_name),
            )
            .with_field("target", TimelineFieldInputValue::String(record.target))
            .with_field("span", TimelineFieldInputValue::String(record.name));
        for field in record.fields {
            input = input.with_field("field", TimelineFieldInputValue::String(field));
        }
        if let Some(source_hwnd) = record.source_hwnd {
            input = input.with_field(
                "source_hwnd",
                TimelineFieldInputValue::String(source_hwnd.to_string()),
            );
        }
        let _ = dataset.push_span(
            input,
            TimelineInstantNs::new(start_ns),
            Some(TimelineInstantNs::new(end_ns)),
        );
    }

    for record in records {
        let at_ns = record
            .timestamp
            .signed_duration_since(first_timestamp)
            .num_nanoseconds()
            .unwrap_or(latest_at_ns)
            .max(0);
        latest_at_ns = latest_at_ns.max(at_ns);
        // timeline[impl playground.live-tracing-thread-identity]
        let input = TimelineItemInput::new(record.message.clone())
            .with_source_key(record.target.clone())
            .with_group_key(record.thread_key.clone())
            .with_field("log_id", TimelineFieldInputValue::U64(record.id))
            .with_field(
                "timestamp",
                TimelineFieldInputValue::String(record.time_text()),
            )
            .with_field(
                "level",
                TimelineFieldInputValue::String(record.level.label().to_owned()),
            )
            .with_field(
                "thread",
                TimelineFieldInputValue::String(record.thread_name),
            )
            .with_field("target", TimelineFieldInputValue::String(record.target))
            .with_field("message", TimelineFieldInputValue::String(record.message))
            .with_field(
                "source_hwnd",
                TimelineFieldInputValue::String(
                    record
                        .source_hwnd
                        .map_or_else(|| "none".to_owned(), |hwnd| hwnd.to_string()),
                ),
            );
        dataset.push_event(input, TimelineInstantNs::new(at_ns));
    }

    let _ = dataset.compact();
    (dataset, latest_at_ns.max(1))
}

pub fn clear_logs() {
    logs_state()
        .records
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
    logs_state()
        .span_records
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
}

#[must_use]
pub fn latest_log_id() -> u64 {
    logs_state()
        .records
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .back()
        .map_or(0, |record| record.id)
}

#[must_use]
pub fn info_log_snapshots_after(last_seen_id: u64) -> Vec<LogRecordSnapshot> {
    logs_state()
        .records
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .filter(|record| record.id > last_seen_id && record.level == LogRecordLevel::Info)
        .cloned()
        .collect()
}

#[must_use]
// observability[impl toasts.levels]
pub fn toast_log_snapshots_after(last_seen_id: u64) -> Vec<LogRecordSnapshot> {
    logs_state()
        .records
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .filter(|record| {
            record.id > last_seen_id
                && matches!(
                    record.level,
                    LogRecordLevel::Info | LogRecordLevel::Warn | LogRecordLevel::Error
                )
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_subscriber::prelude::*;

    static TEST_LOGS_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    // observability[verify logs.capture]
    fn captured_logs_are_returned_oldest_to_newest() {
        let _guard = TEST_LOGS_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        clear_logs();
        let first = push_log_record(
            LogRecordLevel::Info,
            "teamy::test",
            "first".to_owned(),
            Some(42),
        );
        let second = push_log_record(
            LogRecordLevel::Warn,
            "teamy::test",
            "second".to_owned(),
            None,
        );

        let logs = log_snapshots();

        assert_eq!(
            logs.iter().map(|log| log.id).collect::<Vec<_>>(),
            vec![first, second]
        );
        assert_eq!(logs[0].message, "first");
        assert_eq!(logs[0].source_hwnd, Some(42));
        assert_eq!(logs[1].message, "second");
    }

    #[test]
    // observability[verify toasts.levels]
    fn toast_log_query_returns_only_new_user_visible_logs() {
        let _guard = TEST_LOGS_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        clear_logs();
        let first = push_log_record(
            LogRecordLevel::Info,
            "teamy::test",
            "first".to_owned(),
            None,
        );
        let _ = push_log_record(
            LogRecordLevel::Debug,
            "teamy::test",
            "debug".to_owned(),
            None,
        );
        let second = push_log_record(LogRecordLevel::Warn, "teamy::test", "warn".to_owned(), None);
        let third = push_log_record(
            LogRecordLevel::Error,
            "teamy::test",
            "error".to_owned(),
            None,
        );

        let logs = toast_log_snapshots_after(first);

        assert_eq!(
            logs.iter().map(|log| log.id).collect::<Vec<_>>(),
            vec![second, third]
        );
    }

    #[test]
    // observability[verify toasts.levels]
    fn teamy_info_logs_without_source_hwnd_are_toast_visible() {
        let _guard = TEST_LOGS_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        clear_logs();
        let id = push_log_record(
            LogRecordLevel::Info,
            "teamy_studio::app::windows_app",
            "settings opened".to_owned(),
            None,
        );

        let logs = toast_log_snapshots_after(id - 1);

        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].id, id);
        assert_eq!(logs[0].source_hwnd, None);
    }

    #[test]
    // observability[verify logs.capture]
    fn clear_removes_buffered_logs_without_reusing_ids() {
        let _guard = TEST_LOGS_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        clear_logs();
        let before_clear =
            push_log_record(LogRecordLevel::Info, "teamy::test", "old".to_owned(), None);

        clear_logs();
        let after_clear =
            push_log_record(LogRecordLevel::Info, "teamy::test", "new".to_owned(), None);

        assert!(after_clear > before_clear);
        assert_eq!(log_snapshots().len(), 1);
        assert_eq!(log_snapshots()[0].message, "new");
    }

    #[test]
    // observability[verify logs.span-context]
    fn collector_uses_bridged_log_target_and_span_source_hwnd() {
        let _guard = TEST_LOGS_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        clear_logs();
        let mut visitor = LogFieldVisitor::default();
        visitor.record_named_value("log.target", "cubecl_cuda::compute::context".to_owned());
        assert_eq!(
            visitor.target.as_deref(),
            Some("cubecl_cuda::compute::context")
        );

        let subscriber = tracing_subscriber::Registry::default().with(LogCollectorLayer);

        tracing::subscriber::with_default(subscriber, || {
            let _span = tracing::info_span!("scene_window", source_hwnd = 123_isize).entered();
            tracing::trace!("Compiling kernel");
        });

        let logs = log_snapshots();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].source_hwnd, Some(123));
    }

    #[test]
    // observability[verify logs.span-context]
    fn spawn_with_current_span_propagates_source_hwnd() {
        let _guard = TEST_LOGS_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        clear_logs();
        let subscriber = tracing_subscriber::Registry::default().with(LogCollectorLayer);

        tracing::subscriber::with_default(subscriber, || {
            let _span = tracing::info_span!("scene_window", source_hwnd = 456_isize).entered();
            let thread = std::thread::Builder::new()
                .name("trace-worker".to_owned())
                .spawn_with_current_span(|| tracing::info!("worker event"))
                .expect("thread should spawn");
            thread.join().expect("thread should finish");
        });

        let logs = log_snapshots();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].source_hwnd, Some(456));
        assert_eq!(logs[0].thread_name, "trace-worker");
        assert!(logs[0].thread_key.starts_with("trace-worker ThreadId("));

        let (dataset, _) = tracing_event_timeline_dataset();
        let item = dataset.items().first().expect("timeline item");
        assert!(
            dataset
                .resolve_string(item.group_key())
                .is_some_and(|group_key| group_key.starts_with("trace-worker ThreadId("))
        );
    }

    #[test]
    // timeline[verify playground.live-tracing-thread-identity]
    fn collector_keeps_same_named_threads_in_distinct_timeline_rows() {
        let _guard = TEST_LOGS_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        clear_logs();
        let subscriber = tracing_subscriber::Registry::default().with(LogCollectorLayer);

        tracing::subscriber::with_default(subscriber, || {
            let first = std::thread::Builder::new()
                .name("same-name".to_owned())
                .spawn_with_current_span(|| tracing::info!("first"))
                .expect("first thread should spawn");
            let second = std::thread::Builder::new()
                .name("same-name".to_owned())
                .spawn_with_current_span(|| tracing::info!("second"))
                .expect("second thread should spawn");
            first.join().expect("first thread should finish");
            second.join().expect("second thread should finish");
        });

        let (dataset, _) = tracing_event_timeline_dataset();
        let row_keys = dataset
            .items()
            .iter()
            .map(|item| item.group_key())
            .collect::<std::collections::BTreeSet<_>>();

        assert_eq!(row_keys.len(), 2);
    }

    #[test]
    // timeline[verify playground.live-tracing-spans]
    fn collector_projects_closed_tracing_spans_as_timeline_spans() {
        let _guard = TEST_LOGS_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        clear_logs();
        let subscriber = tracing_subscriber::Registry::default().with(LogCollectorLayer);

        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!("outer_work", source_hwnd = 789_isize);
            let _entered = span.enter();
        });

        let spans = log_span_snapshots();
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].name, "outer_work");
        assert_eq!(spans[0].source_hwnd, Some(789));

        let (dataset, _) = tracing_event_timeline_dataset();
        assert!(dataset.items().iter().any(|item| {
            matches!(item.kind(), crate::timeline::TimelineItemKind::Span(span) if span.end().is_some())
        }));
    }
}
