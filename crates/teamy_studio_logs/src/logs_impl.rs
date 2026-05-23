use chrono::{DateTime, Local};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{Builder, JoinHandle};
use tracing::field::{Field, Visit};
use tracing::{Event, Id, Level, Metadata, Subscriber, span};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;

use crate::timeline::{
    TimelineDataset, TimelineFieldInputValue, TimelineInstantNs, TimelineItemId,
    TimelineItemInput,
};

const INITIAL_LOG_RECORD_CAPACITY: usize = 4_096;
const INITIAL_LOG_SPAN_RECORD_CAPACITY: usize = 4_096;

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
    pub is_tracy_frame_mark: bool,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogActiveSpanSnapshot {
    pub id: u64,
    pub start_timestamp: DateTime<Local>,
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
    active_span_revision: AtomicU64,
    records: Mutex<VecDeque<LogRecordSnapshot>>,
    span_records: Mutex<VecDeque<LogSpanSnapshot>>,
    active_spans: Mutex<HashMap<u64, LogActiveSpanSnapshot>>,
    timeline_records: Mutex<Vec<LogRecordSnapshot>>,
    timeline_span_records: Mutex<Vec<LogSpanSnapshot>>,
    timeline_cache: Mutex<LiveTracingTimelineCache>,
}

impl Default for LogsState {
    fn default() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            next_span_id: AtomicU64::new(1),
            active_span_revision: AtomicU64::new(0),
            records: Mutex::new(VecDeque::with_capacity(INITIAL_LOG_RECORD_CAPACITY)),
            span_records: Mutex::new(VecDeque::with_capacity(INITIAL_LOG_SPAN_RECORD_CAPACITY)),
            active_spans: Mutex::new(HashMap::new()),
            timeline_records: Mutex::new(Vec::with_capacity(INITIAL_LOG_RECORD_CAPACITY)),
            timeline_span_records: Mutex::new(Vec::with_capacity(INITIAL_LOG_SPAN_RECORD_CAPACITY)),
            timeline_cache: Mutex::new(LiveTracingTimelineCache::default()),
        }
    }
}

#[derive(Clone, Debug, Default)]
struct LiveTracingTimelineCache {
    first_log_id: u64,
    latest_log_id: u64,
    first_span_id: u64,
    latest_span_id: u64,
    record_count: usize,
    span_count: usize,
    active_span_count: usize,
    active_span_revision: u64,
    first_timestamp: Option<DateTime<Local>>,
    dataset: Arc<TimelineDataset>,
    latest_at_ns: i64,
    active_span_item_ids: HashMap<u64, TimelineItemId>,
    active_spans: HashMap<u64, LogActiveSpanSnapshot>,
}

impl LiveTracingTimelineCache {
    fn matches_revision(&self, revision: LiveTracingSnapshotRevision) -> bool {
        self.latest_log_id == revision.latest_log_id
            && self.latest_span_id == revision.latest_span_id
            && self.record_count == revision.record_count
            && self.span_count == revision.span_count
            && self.active_span_count == revision.active_span_count
            && self.active_span_revision == revision.active_span_revision
    }

    fn can_append(
        &self,
        revision: LiveTracingSnapshotRevision,
        first_log_id: u64,
        first_span_id: u64,
        first_timestamp: Option<&DateTime<Local>>,
    ) -> bool {
        self.record_count <= revision.record_count
            && self.span_count <= revision.span_count
            && (self.record_count == 0 || self.first_log_id == first_log_id)
            && (self.span_count == 0 || self.first_span_id == first_span_id)
            && self.first_timestamp.as_ref() == first_timestamp
    }

    fn store(
        &mut self,
        revision: LiveTracingSnapshotRevision,
        first_log_id: u64,
        first_span_id: u64,
        first_timestamp: Option<DateTime<Local>>,
        dataset: Arc<TimelineDataset>,
        latest_at_ns: i64,
        active_span_item_ids: HashMap<u64, TimelineItemId>,
        active_spans: HashMap<u64, LogActiveSpanSnapshot>,
    ) {
        self.first_log_id = first_log_id;
        self.latest_log_id = revision.latest_log_id;
        self.first_span_id = first_span_id;
        self.latest_span_id = revision.latest_span_id;
        self.record_count = revision.record_count;
        self.span_count = revision.span_count;
        self.active_span_count = revision.active_span_count;
        self.active_span_revision = revision.active_span_revision;
        self.first_timestamp = first_timestamp;
        self.dataset = dataset;
        self.latest_at_ns = latest_at_ns;
        self.active_span_item_ids = active_span_item_ids;
        self.active_spans = active_spans;
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LiveTracingSnapshotRevision {
    latest_log_id: u64,
    latest_span_id: u64,
    record_count: usize,
    span_count: usize,
    active_span_count: usize,
    active_span_revision: u64,
}

impl LiveTracingSnapshotRevision {
    #[must_use]
    pub const fn record_count(self) -> usize {
        self.record_count
    }

    #[must_use]
    pub const fn span_count(self) -> usize {
        self.span_count
    }

    #[must_use]
    pub const fn active_span_count(self) -> usize {
        self.active_span_count
    }

    #[must_use]
    pub const fn has_active_spans(self) -> bool {
        self.active_span_count > 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveTracingSnapshotDelta {
    None,
    FrameMarksOnly,
    Other,
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
    timeline_span_id: u64,
    start_timestamp: DateTime<Local>,
    thread_name: String,
    thread_key: String,
    target: String,
    name: String,
    fields: Vec<String>,
    source_hwnd: Option<isize>,
    collect_timeline_span: bool,
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
        let fields = LogSpanFields {
            timeline_span_id: logs_state().next_span_id.fetch_add(1, Ordering::AcqRel),
            start_timestamp: Local::now(),
            thread_name: current_thread_name(),
            thread_key: current_thread_key(),
            target: attrs.metadata().target().to_owned(),
            name: attrs.metadata().name().to_owned(),
            fields: visitor.fields,
            source_hwnd: visitor.source_hwnd,
            collect_timeline_span: live_timeline_should_collect_span(attrs.metadata()),
        };
        if fields.collect_timeline_span {
            update_active_span_record(&fields);
        }
        span.extensions_mut().replace(fields);
    }

    // timeline[impl playground.live-tracing-spans]
    fn on_close(&self, id: Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(&id) else {
            return;
        };
        let Some(fields) = span.extensions().get::<LogSpanFields>().cloned() else {
            return;
        };
        if !fields.collect_timeline_span {
            return;
        }
        remove_active_span_record(fields.timeline_span_id);
        push_log_span_record(fields, Local::now());
    }

    fn on_enter(&self, id: &Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else {
            return;
        };
        let mut extensions = span.extensions_mut();
        let updated_fields = if let Some(fields) = extensions.get_mut::<LogSpanFields>() {
            fields.thread_name = current_thread_name();
            fields.thread_key = current_thread_key();
            fields.collect_timeline_span.then_some(fields.clone())
        } else {
            None
        };
        drop(extensions);
        if let Some(fields) = updated_fields.as_ref() {
            update_active_span_record(fields);
        }
    }

    fn on_record(&self, id: &Id, values: &span::Record<'_>, ctx: Context<'_, S>) {
        let mut visitor = LogFieldVisitor::default();
        values.record(&mut visitor);
        let Some(span) = ctx.span(id) else {
            return;
        };
        let mut extensions = span.extensions_mut();
        let updated_fields = if let Some(fields) = extensions.get_mut::<LogSpanFields>() {
            if let Some(source_hwnd) = visitor.source_hwnd {
                fields.source_hwnd = Some(source_hwnd);
            }
            fields.fields.extend(visitor.fields);
            fields.collect_timeline_span.then_some(fields.clone())
        } else if let Some(source_hwnd) = visitor.source_hwnd {
            let fields = LogSpanFields {
                timeline_span_id: logs_state().next_span_id.fetch_add(1, Ordering::AcqRel),
                start_timestamp: Local::now(),
                thread_name: current_thread_name(),
                thread_key: current_thread_key(),
                target: String::new(),
                name: "span".to_owned(),
                fields: visitor.fields,
                source_hwnd: Some(source_hwnd),
                collect_timeline_span: true,
            };
            extensions.replace(fields.clone());
            Some(fields)
        } else {
            None
        };
        drop(extensions);
        if let Some(fields) = updated_fields.as_ref() {
            update_active_span_record(fields);
        }
    }

    // observability[impl logs.capture]
    // observability[impl logs.span-context]
    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        if !live_timeline_should_collect_event(event.metadata()) {
            return;
        }
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
            event_is_tracy_frame_mark(metadata),
        );
    }
}

fn live_timeline_should_collect_event(metadata: &Metadata<'_>) -> bool {
    if metadata.target() == "timeline_playground_profiling" {
        return false;
    }
    !matches!(
        metadata.target(),
        "tracy_client" | "tracy_client::client" | "tracing_tracy" | "tracing_tracy::layer"
    )
}

fn event_is_tracy_frame_mark(metadata: &Metadata<'_>) -> bool {
    metadata.fields().field("tracy.frame_mark").is_some()
}

fn live_timeline_is_internal_window_span(metadata: &Metadata<'_>) -> bool {
    metadata.target() == "teamy_studio_app_host::windows_app_impl"
        && matches!(
            metadata.name(),
            "scene_window"
                | "scene_focused_render_tick"
                | "wait_for_window_message"
                | "dispatch_pending_thread_messages"
                | "render_focused_animation_frame"
                | "compute_client_layout"
                | "build_diagnostic_panel_text"
                | "build_terminal_display_state"
                | "submit_render_frame_model"
        )
}

fn live_timeline_should_collect_span(metadata: &Metadata<'_>) -> bool {
    if metadata.target() == "timeline_playground_profiling"
        || metadata.name().starts_with("timeline_playground_")
        || live_timeline_is_internal_window_span(metadata)
    {
        return false;
    }

    !matches!(
        metadata.name(),
        "render_thread_render_frame"
            | "render_thread_resize_swap_chain"
            | "update_slug_curves"
            | "update_scene_vertices"
            | "wait_for_frame_sync"
            | "wait_for_frame_fence"
            | "record_render_commands"
            | "submit_and_present_frame"
    )
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
    is_tracy_frame_mark: bool,
) -> u64 {
    let state = logs_state();
    let id = state.next_id.fetch_add(1, Ordering::AcqRel);
    let mut records = state
        .records
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    records.push_back(LogRecordSnapshot {
        id,
        timestamp: Local::now(),
        level,
        thread_name: current_thread_name(),
        thread_key: current_thread_key(),
        target: target.to_owned(),
        message,
        source_hwnd,
        is_tracy_frame_mark,
    });
    let snapshot = records
        .back()
        .cloned()
        .expect("newly pushed log record should exist");
    drop(records);
    state
        .timeline_records
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(snapshot);
    id
}

fn push_log_span_record(fields: LogSpanFields, end_timestamp: DateTime<Local>) -> u64 {
    let state = logs_state();
    let id = fields.timeline_span_id;
    let mut span_records = state
        .span_records
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
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
    let snapshot = span_records
        .back()
        .cloned()
        .expect("newly pushed span record should exist");
    drop(span_records);
    state
        .timeline_span_records
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(snapshot);
    id
}

fn update_active_span_record(fields: &LogSpanFields) {
    let state = logs_state();
    let next_snapshot = LogActiveSpanSnapshot {
        id: fields.timeline_span_id,
        start_timestamp: fields.start_timestamp,
        thread_name: fields.thread_name.clone(),
        thread_key: fields.thread_key.clone(),
        target: fields.target.clone(),
        name: fields.name.clone(),
        fields: fields.fields.clone(),
        source_hwnd: fields.source_hwnd,
    };
    let changed = state
        .active_spans
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(fields.timeline_span_id, next_snapshot.clone())
        .as_ref()
        != Some(&next_snapshot);
    if changed {
        let _ = state.active_span_revision.fetch_add(1, Ordering::AcqRel);
    }
}

fn remove_active_span_record(id: u64) {
    let state = logs_state();
    let removed = state
        .active_spans
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&id)
        .is_some();
    if removed {
        let _ = state.active_span_revision.fetch_add(1, Ordering::AcqRel);
    }
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
pub fn active_span_snapshots() -> Vec<LogActiveSpanSnapshot> {
    let mut spans = logs_state()
        .active_spans
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .values()
        .cloned()
        .collect::<Vec<_>>();
    spans.sort_by_key(|span| span.id);
    spans
}

#[must_use]
pub fn live_tracing_snapshot_revision() -> LiveTracingSnapshotRevision {
    let records = logs_state()
        .timeline_records
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let latest_log_id = records.last().map_or(0, |record| record.id);
    let record_count = records.len();
    drop(records);

    let span_records = logs_state()
        .timeline_span_records
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let active_span_count = logs_state()
        .active_spans
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .len();
    LiveTracingSnapshotRevision {
        latest_log_id,
        latest_span_id: span_records.last().map_or(0, |record| record.id),
        record_count,
        span_count: span_records.len(),
        active_span_count,
        active_span_revision: logs_state().active_span_revision.load(Ordering::Acquire),
    }
}

#[must_use]
pub fn live_tracing_snapshot_delta_since(
    previous: Option<LiveTracingSnapshotRevision>,
) -> LiveTracingSnapshotDelta {
    let Some(previous) = previous else {
        return LiveTracingSnapshotDelta::Other;
    };

    let current = live_tracing_snapshot_revision();
    if current == previous {
        return LiveTracingSnapshotDelta::None;
    }

    if current.latest_span_id != previous.latest_span_id
        || current.span_count != previous.span_count
        || current.active_span_count != previous.active_span_count
        || current.active_span_revision != previous.active_span_revision
        || current.record_count < previous.record_count
        || current.latest_log_id < previous.latest_log_id
    {
        return LiveTracingSnapshotDelta::Other;
    }

    let records = logs_state()
        .timeline_records
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut saw_frame_mark = false;
    for record in records.iter().skip(previous.record_count) {
        if !record.is_tracy_frame_mark {
            return LiveTracingSnapshotDelta::Other;
        }
        saw_frame_mark = true;
    }

    if saw_frame_mark {
        LiveTracingSnapshotDelta::FrameMarksOnly
    } else {
        LiveTracingSnapshotDelta::Other
    }
}

#[must_use]
// timeline[impl playground.live-tracing-events]
pub fn tracing_event_timeline_dataset() -> (Arc<TimelineDataset>, i64) {
    #[cfg(feature = "extended_observability")]
    let _span = tracing::debug_span!(
        target: "timeline_playground_profiling",
        "timeline_playground_live_tracing_snapshot_state"
    )
    .entered();
    let state = logs_state();
    let records = state
        .timeline_records
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let span_records = state
        .timeline_span_records
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let active_spans = state
        .active_spans
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let revision = LiveTracingSnapshotRevision {
        latest_log_id: records.last().map_or(0, |record| record.id),
        latest_span_id: span_records.last().map_or(0, |record| record.id),
        record_count: records.len(),
        span_count: span_records.len(),
        active_span_count: active_spans.len(),
        active_span_revision: state.active_span_revision.load(Ordering::Acquire),
    };
    let first_log_id = records.first().map_or(0, |record| record.id);
    let first_active_span_id = active_spans.values().map(|record| record.id).min();
    let first_span_id = match (span_records.first(), first_active_span_id) {
        (Some(closed), Some(active)) => closed.id.min(active),
        (Some(closed), None) => closed.id,
        (None, Some(active)) => active,
        (None, None) => 0,
    };
    let first_timestamp = live_tracing_first_timestamp(
        records.iter(),
        span_records.iter(),
        active_spans.values(),
    );
    let mut cache = state
        .timeline_cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if cache.matches_revision(revision) {
        #[cfg(feature = "extended_observability")]
        let _span = tracing::debug_span!(
            target: "timeline_playground_profiling",
            "timeline_playground_live_tracing_cache_hit"
        )
        .entered();
        let latest_at_ns = if active_spans.is_empty() {
            cache.latest_at_ns
        } else {
            cache.latest_at_ns.max(current_live_tracing_time_ns(
                first_timestamp.expect("active spans require a first timestamp"),
            ))
        };
        return (cache.dataset.clone(), latest_at_ns.max(1));
    }
    let new_closed_span_ids = span_records
        .iter()
        .skip(cache.span_count)
        .map(|record| record.id)
        .collect::<HashSet<_>>();
    let active_span_changes_require_rebuild = cache.active_spans.iter().any(|(id, cached)| {
        active_spans.get(id).is_some_and(|current| current != cached)
            || (!active_spans.contains_key(id) && !new_closed_span_ids.contains(id))
    });

    if cache.can_append(
        revision,
        first_log_id,
        first_span_id,
        first_timestamp.as_ref(),
    ) && !active_span_changes_require_rebuild
    {
        #[cfg(feature = "extended_observability")]
        let _span = tracing::debug_span!(
            target: "timeline_playground_profiling",
            "timeline_playground_live_tracing_append_dataset"
        )
        .entered();
        let cached_record_count = cache.record_count;
        let cached_span_count = cache.span_count;
        let mut latest_at_ns = cache.latest_at_ns.max(1);
        let mut active_span_item_ids = cache.active_span_item_ids.clone();
        let first_timestamp =
            first_timestamp.expect("append-only live tracing cache retains its first timestamp");
        let dataset = Arc::make_mut(&mut cache.dataset);

        {
            #[cfg(feature = "extended_observability")]
            let _span = tracing::debug_span!(
                target: "timeline_playground_profiling",
                "timeline_playground_live_tracing_append_closed_spans"
            )
            .entered();
            for record in span_records.iter().skip(cached_span_count) {
                if let Some(item_id) = active_span_item_ids.remove(&record.id) {
                    latest_at_ns = latest_at_ns.max(finish_live_tracing_active_span_timeline_item(
                        dataset,
                        item_id,
                        record,
                        first_timestamp,
                        latest_at_ns,
                    ));
                } else {
                    latest_at_ns = latest_at_ns.max(push_live_tracing_span_timeline_item(
                        dataset,
                        record,
                        first_timestamp,
                        latest_at_ns,
                    ));
                }
            }
        }

        {
            #[cfg(feature = "extended_observability")]
            let _span = tracing::debug_span!(
                target: "timeline_playground_profiling",
                "timeline_playground_live_tracing_append_events"
            )
            .entered();
            for record in records.iter().skip(cached_record_count) {
                latest_at_ns = latest_at_ns.max(push_live_tracing_event_timeline_item(
                    dataset,
                    record,
                    first_timestamp,
                    latest_at_ns,
                ));
            }
        }

        {
            #[cfg(feature = "extended_observability")]
            let _span = tracing::debug_span!(
                target: "timeline_playground_profiling",
                "timeline_playground_live_tracing_append_active_spans"
            )
            .entered();
            let mut sorted_active_spans = active_spans.values().collect::<Vec<_>>();
            sorted_active_spans.sort_by_key(|record| record.id);
            for record in sorted_active_spans {
                if active_span_item_ids.contains_key(&record.id) {
                    continue;
                }
                let (item_id, start_ns) = push_live_tracing_active_span_timeline_item(
                    dataset,
                    record,
                    first_timestamp,
                    latest_at_ns,
                );
                latest_at_ns = latest_at_ns.max(start_ns);
                active_span_item_ids.insert(record.id, item_id);
            }
            if !active_spans.is_empty() {
                latest_at_ns = latest_at_ns.max(current_live_tracing_time_ns(first_timestamp));
            }
        }
        let latest_at_ns = latest_at_ns.max(1);
        let dataset = cache.dataset.clone();
        cache.store(
            revision,
            first_log_id,
            first_span_id,
            Some(first_timestamp),
            dataset.clone(),
            latest_at_ns,
            active_span_item_ids,
            active_spans.clone(),
        );
        let (dataset, latest_at_ns) = (dataset, latest_at_ns);
        return (dataset, latest_at_ns);
    }

    let mut dataset = TimelineDataset::new();
    let Some(first_timestamp) = first_timestamp else {
        let dataset = Arc::new(dataset);
        cache.store(
            revision,
            first_log_id,
            first_span_id,
            None,
            dataset.clone(),
            1,
            HashMap::new(),
            HashMap::new(),
        );
        return (dataset, 1);
    };
    let mut latest_at_ns = 1_i64;
    let mut active_span_item_ids = HashMap::new();

    {
        #[cfg(feature = "extended_observability")]
        let _span = tracing::debug_span!(
            target: "timeline_playground_profiling",
            "timeline_playground_live_tracing_rebuild_dataset"
        )
        .entered();

        {
            #[cfg(feature = "extended_observability")]
            let _span = tracing::debug_span!(
                target: "timeline_playground_profiling",
                "timeline_playground_live_tracing_rebuild_closed_spans"
            )
            .entered();
            for record in span_records.iter() {
                latest_at_ns = latest_at_ns.max(push_live_tracing_span_timeline_item(
                    &mut dataset,
                    record,
                    first_timestamp,
                    latest_at_ns,
                ));
            }
        }

        {
            #[cfg(feature = "extended_observability")]
            let _span = tracing::debug_span!(
                target: "timeline_playground_profiling",
                "timeline_playground_live_tracing_rebuild_events"
            )
            .entered();
            for record in records.iter() {
                latest_at_ns = latest_at_ns.max(push_live_tracing_event_timeline_item(
                    &mut dataset,
                    record,
                    first_timestamp,
                    latest_at_ns,
                ));
            }
        }

        {
            #[cfg(feature = "extended_observability")]
            let _span = tracing::debug_span!(
                target: "timeline_playground_profiling",
                "timeline_playground_live_tracing_rebuild_active_spans"
            )
            .entered();
            let mut sorted_active_spans = active_spans.values().collect::<Vec<_>>();
            sorted_active_spans.sort_by_key(|record| record.id);
            for record in sorted_active_spans {
                let (item_id, start_ns) = push_live_tracing_active_span_timeline_item(
                    &mut dataset,
                    record,
                    first_timestamp,
                    latest_at_ns,
                );
                latest_at_ns = latest_at_ns.max(start_ns);
                active_span_item_ids.insert(record.id, item_id);
            }
        }
    }

    if revision.active_span_count > 0 {
        latest_at_ns = latest_at_ns.max(current_live_tracing_time_ns(first_timestamp));
    }

    let latest_at_ns = latest_at_ns.max(1);
    let dataset = Arc::new(dataset);
    cache.store(
        revision,
        first_log_id,
        first_span_id,
        Some(first_timestamp),
        dataset.clone(),
        latest_at_ns,
        active_span_item_ids,
        active_spans.clone(),
    );
    (dataset, latest_at_ns)
}

fn live_tracing_first_timestamp<'a>(
    records: impl IntoIterator<Item = &'a LogRecordSnapshot>,
    span_records: impl IntoIterator<Item = &'a LogSpanSnapshot>,
    active_spans: impl IntoIterator<Item = &'a LogActiveSpanSnapshot>,
) -> Option<DateTime<Local>> {
    records
        .into_iter()
        .map(|record| record.timestamp)
        .chain(span_records.into_iter().map(|record| record.start_timestamp))
        .chain(active_spans.into_iter().map(|record| record.start_timestamp))
        .min()
}

fn current_live_tracing_time_ns(first_timestamp: DateTime<Local>) -> i64 {
    Local::now()
        .signed_duration_since(first_timestamp)
        .num_nanoseconds()
        .unwrap_or(1)
        .max(1)
}

fn push_live_tracing_span_timeline_item(
    dataset: &mut TimelineDataset,
    record: &LogSpanSnapshot,
    first_timestamp: DateTime<Local>,
    fallback_at_ns: i64,
) -> i64 {
    let start_ns = record
        .start_timestamp
        .signed_duration_since(first_timestamp)
        .num_nanoseconds()
        .unwrap_or(fallback_at_ns)
        .max(0);
    let end_ns = record
        .end_timestamp
        .signed_duration_since(first_timestamp)
        .num_nanoseconds()
        .unwrap_or(start_ns)
        .max(start_ns);
    // timeline[impl playground.live-tracing-thread-identity]
    let mut input = TimelineItemInput::new(record.name.clone())
        .with_source_key(record.target.clone())
        .with_group_key(record.thread_key.clone())
        .with_field("span_id", TimelineFieldInputValue::U64(record.id))
        .with_field(
            "thread",
            TimelineFieldInputValue::String(record.thread_name.clone()),
        )
        .with_field("target", TimelineFieldInputValue::String(record.target.clone()))
        .with_field("span", TimelineFieldInputValue::String(record.name.clone()));
    for field in &record.fields {
        input = input.with_field("field", TimelineFieldInputValue::String(field.clone()));
    }
    if let Some(source_hwnd) = record.source_hwnd {
        input = input.with_field(
            "source_hwnd",
            TimelineFieldInputValue::String(source_hwnd.to_string()),
        );
    }
    let _ = dataset.push_span_indexed(
        input,
        TimelineInstantNs::new(start_ns),
        Some(TimelineInstantNs::new(end_ns)),
    )
    .expect("live tracing span timestamps are ordered");
    end_ns
}

fn finish_live_tracing_active_span_timeline_item(
    dataset: &mut TimelineDataset,
    item_id: TimelineItemId,
    record: &LogSpanSnapshot,
    first_timestamp: DateTime<Local>,
    fallback_at_ns: i64,
) -> i64 {
    let end_ns = record
        .end_timestamp
        .signed_duration_since(first_timestamp)
        .num_nanoseconds()
        .unwrap_or(fallback_at_ns)
        .max(0);
    dataset
        .finish_span_indexed(item_id, TimelineInstantNs::new(end_ns))
        .expect("live tracing span timestamps are ordered");
    end_ns
}

fn push_live_tracing_event_timeline_item(
    dataset: &mut TimelineDataset,
    record: &LogRecordSnapshot,
    first_timestamp: DateTime<Local>,
    fallback_at_ns: i64,
) -> i64 {
    let at_ns = record
        .timestamp
        .signed_duration_since(first_timestamp)
        .num_nanoseconds()
        .unwrap_or(fallback_at_ns)
        .max(0);
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
            TimelineFieldInputValue::String(record.thread_name.clone()),
        )
        .with_field("target", TimelineFieldInputValue::String(record.target.clone()))
        .with_field("message", TimelineFieldInputValue::String(record.message.clone()))
        .with_field(
            "source_hwnd",
            TimelineFieldInputValue::String(
                record
                    .source_hwnd
                    .map_or_else(|| "none".to_owned(), |hwnd| hwnd.to_string()),
            ),
        );
    dataset.push_event_indexed(input, TimelineInstantNs::new(at_ns));
    at_ns
}

fn push_live_tracing_active_span_timeline_item(
    dataset: &mut TimelineDataset,
    record: &LogActiveSpanSnapshot,
    first_timestamp: DateTime<Local>,
    fallback_at_ns: i64,
) -> (TimelineItemId, i64) {
    let start_ns = record
        .start_timestamp
        .signed_duration_since(first_timestamp)
        .num_nanoseconds()
        .unwrap_or(fallback_at_ns)
        .max(0);
    let mut input = TimelineItemInput::new(record.name.clone())
        .with_source_key(record.target.clone())
        .with_group_key(record.thread_key.clone())
        .with_field("span_id", TimelineFieldInputValue::U64(record.id))
        .with_field(
            "thread",
            TimelineFieldInputValue::String(record.thread_name.clone()),
        )
        .with_field("target", TimelineFieldInputValue::String(record.target.clone()))
        .with_field("span", TimelineFieldInputValue::String(record.name.clone()));
    for field in &record.fields {
        input = input.with_field("field", TimelineFieldInputValue::String(field.clone()));
    }
    if let Some(source_hwnd) = record.source_hwnd {
        input = input.with_field(
            "source_hwnd",
            TimelineFieldInputValue::String(source_hwnd.to_string()),
        );
    }
    let item_id = dataset
        .push_span_indexed(input, TimelineInstantNs::new(start_ns), None)
        .expect("live tracing span timestamps are ordered");
    (item_id, start_ns)
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
    logs_state()
        .active_spans
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
    logs_state()
        .timeline_records
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
    logs_state()
        .timeline_span_records
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
    logs_state().active_span_revision.store(0, Ordering::Release);
    *logs_state()
        .timeline_cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = LiveTracingTimelineCache::default();
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
                && !record.is_tracy_frame_mark
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
            false,
        );
        let second = push_log_record(
            LogRecordLevel::Warn,
            "teamy::test",
            "second".to_owned(),
            None,
            false,
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
            false,
        );
        let _ = push_log_record(
            LogRecordLevel::Debug,
            "teamy::test",
            "debug".to_owned(),
            None,
            false,
        );
        let second = push_log_record(
            LogRecordLevel::Warn,
            "teamy::test",
            "warn".to_owned(),
            None,
            false,
        );
        let third = push_log_record(
            LogRecordLevel::Error,
            "teamy::test",
            "error".to_owned(),
            None,
            false,
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
            false,
        );

        let logs = toast_log_snapshots_after(id - 1);

        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].id, id);
        assert_eq!(logs[0].source_hwnd, None);
    }

    #[test]
    // observability[verify toasts.levels]
    fn tracy_frame_mark_logs_are_not_toast_visible() {
        let _guard = TEST_LOGS_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        clear_logs();
        let id = push_log_record(
            LogRecordLevel::Info,
            "teamy_studio_shell::windows_d3d12_renderer",
            "finished frame".to_owned(),
            Some(42),
            true,
        );

        let logs = toast_log_snapshots_after(id - 1);

        assert!(logs.is_empty());
    }

    #[test]
    // observability[verify logs.capture]
    fn clear_removes_buffered_logs_without_reusing_ids() {
        let _guard = TEST_LOGS_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        clear_logs();
        let before_clear = push_log_record(
            LogRecordLevel::Info,
            "teamy::test",
            "old".to_owned(),
            None,
            false,
        );

        clear_logs();
        let after_clear = push_log_record(
            LogRecordLevel::Info,
            "teamy::test",
            "new".to_owned(),
            None,
            false,
        );

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
            .map(crate::timeline::dataset::TimelineItem::group_key)
            .collect::<std::collections::BTreeSet<_>>();

        assert_eq!(row_keys.len(), 2);
    }

    #[test]
    fn live_timeline_dataset_reuses_cached_snapshot_when_records_do_not_change() {
        let _guard = TEST_LOGS_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        clear_logs();
        let _ = push_log_record(
            LogRecordLevel::Info,
            "teamy::test",
            "first".to_owned(),
            None,
            false,
        );

        let (first_dataset, _) = tracing_event_timeline_dataset();
        let (second_dataset, _) = tracing_event_timeline_dataset();

        assert_eq!(first_dataset.items().len(), second_dataset.items().len());
        assert_eq!(first_dataset.revision(), second_dataset.revision());
    }

    #[test]
    fn live_timeline_append_keeps_indexes_current() {
        let _guard = TEST_LOGS_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        clear_logs();
        let _ = push_log_record(
            LogRecordLevel::Info,
            "teamy::test",
            "first".to_owned(),
            None,
            false,
        );
        let _ = tracing_event_timeline_dataset();
        let _ = push_log_record(
            LogRecordLevel::Info,
            "teamy::test",
            "second".to_owned(),
            None,
            false,
        );

        let (dataset, _) = tracing_event_timeline_dataset();

        assert_eq!(dataset.items().len(), 2);
        assert_eq!(dataset.pending_write_count(), 0);
        assert_eq!(dataset.index_revision(), dataset.revision());
    }

    #[test]
    fn live_timeline_cache_allows_append_only_growth() {
        let base = Local::now();
        let cache = LiveTracingTimelineCache {
            first_log_id: 41,
            latest_log_id: 41,
            record_count: 1,
            first_timestamp: Some(base),
            ..LiveTracingTimelineCache::default()
        };

        assert!(cache.can_append(
            LiveTracingSnapshotRevision {
                latest_log_id: 42,
                latest_span_id: 0,
                record_count: 2,
                span_count: 0,
                active_span_count: 0,
                active_span_revision: 0,
            },
            41,
            0,
            Some(&base),
        ));
    }

    #[test]
    fn live_timeline_cache_allows_active_span_churn_during_append() {
        let base = Local::now();
        let cache = LiveTracingTimelineCache {
            first_log_id: 41,
            latest_log_id: 41,
            first_span_id: 7,
            latest_span_id: 7,
            record_count: 1,
            span_count: 1,
            active_span_count: 1,
            active_span_revision: 3,
            first_timestamp: Some(base),
            ..LiveTracingTimelineCache::default()
        };

        assert!(cache.can_append(
            LiveTracingSnapshotRevision {
                latest_log_id: 42,
                latest_span_id: 8,
                record_count: 2,
                span_count: 2,
                active_span_count: 2,
                active_span_revision: 4,
            },
            41,
            7,
            Some(&base),
        ));
    }

    #[test]
    fn live_timeline_retains_spans_beyond_initial_capacity() {
        let _guard = TEST_LOGS_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        clear_logs();
        let base = Local::now();

        for index in 0..=INITIAL_LOG_SPAN_RECORD_CAPACITY {
            let start_timestamp = base + chrono::TimeDelta::nanoseconds(index as i64);
            let end_timestamp = start_timestamp + chrono::TimeDelta::nanoseconds(1);
            let _ = push_log_span_record(
                LogSpanFields {
                    timeline_span_id: u64::try_from(index + 1).expect("span id"),
                    start_timestamp,
                    thread_name: "timeline-test".to_owned(),
                    thread_key: format!("timeline-test {index}"),
                    target: "teamy::test".to_owned(),
                    name: format!("span-{index}"),
                    fields: Vec::new(),
                    source_hwnd: None,
                    collect_timeline_span: true,
                },
                end_timestamp,
            );
        }

        assert_eq!(log_span_snapshots().len(), INITIAL_LOG_SPAN_RECORD_CAPACITY + 1);

        let (dataset, _) = tracing_event_timeline_dataset();
        assert_eq!(dataset.items().len(), INITIAL_LOG_SPAN_RECORD_CAPACITY + 1);
    }

    #[test]
    fn live_timeline_incrementally_closes_active_spans_without_duplicates() {
        let _guard = TEST_LOGS_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        clear_logs();
        let start_timestamp = Local::now();
        let end_timestamp = start_timestamp + chrono::TimeDelta::nanoseconds(10);
        let fields = LogSpanFields {
            timeline_span_id: 9_001,
            start_timestamp,
            thread_name: "timeline-test".to_owned(),
            thread_key: "timeline-test 9001".to_owned(),
            target: "teamy::test".to_owned(),
            name: "span-9001".to_owned(),
            fields: Vec::new(),
            source_hwnd: None,
            collect_timeline_span: true,
        };

        update_active_span_record(&fields);
        let (open_dataset, _) = tracing_event_timeline_dataset();
        assert_eq!(open_dataset.items().len(), 1);

        remove_active_span_record(fields.timeline_span_id);
        let _ = push_log_span_record(fields, end_timestamp);

        let (closed_dataset, _) = tracing_event_timeline_dataset();
        assert_eq!(closed_dataset.items().len(), 1);
        let crate::timeline::TimelineItemKind::Span(span) =
            closed_dataset.items()[0].kind()
        else {
            panic!("expected span item");
        };
        assert_eq!(span.end(), Some(TimelineInstantNs::new(10)));
    }

    #[test]
    fn live_timeline_cache_rejects_ring_buffer_rollover() {
        let base = Local::now();
        let cache = LiveTracingTimelineCache {
            first_log_id: 41,
            latest_log_id: 4_136,
            record_count: 4_096,
            first_timestamp: Some(base),
            ..LiveTracingTimelineCache::default()
        };

        assert!(!cache.can_append(
            LiveTracingSnapshotRevision {
                latest_log_id: 4_137,
                latest_span_id: 0,
                record_count: 4_096,
                span_count: 0,
                active_span_count: 0,
                active_span_revision: 0,
            },
            42,
            0,
            Some(&base),
        ));
    }

    #[test]
    fn live_timeline_cache_rejects_earlier_first_timestamp() {
        let base = Local::now();
        let earlier = base - chrono::TimeDelta::milliseconds(1);
        let cache = LiveTracingTimelineCache {
            first_log_id: 41,
            latest_log_id: 41,
            record_count: 1,
            first_timestamp: Some(base),
            ..LiveTracingTimelineCache::default()
        };

        assert!(!cache.can_append(
            LiveTracingSnapshotRevision {
                latest_log_id: 42,
                latest_span_id: 0,
                record_count: 2,
                span_count: 0,
                active_span_count: 0,
                active_span_revision: 0,
            },
            41,
            0,
            Some(&earlier),
        ));
    }

    #[test]
    fn collector_ignores_high_frequency_self_profiler_spans() {
        let _guard = TEST_LOGS_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        clear_logs();
        let subscriber = tracing_subscriber::Registry::default().with(LogCollectorLayer);

        tracing::subscriber::with_default(subscriber, || {
            let _ignored = tracing::debug_span!("render_thread_render_frame").entered();
        });

        assert!(log_span_snapshots().is_empty());
    }

    #[test]
    fn collector_ignores_internal_scene_window_spans() {
        let _guard = TEST_LOGS_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        clear_logs();
        let subscriber = tracing_subscriber::Registry::default().with(LogCollectorLayer);

        tracing::subscriber::with_default(subscriber, || {
            let _scene_window = tracing::info_span!(
                target: "teamy_studio_app_host::windows_app_impl",
                "scene_window",
                source_hwnd = 321_isize
            )
            .entered();
            let _render_tick = tracing::debug_span!(
                target: "teamy_studio_app_host::windows_app_impl",
                "scene_focused_render_tick"
            )
            .entered();
        });

        assert!(log_span_snapshots().is_empty());

        let (dataset, _) = tracing_event_timeline_dataset();
        assert!(dataset.items().is_empty());
    }

    #[test]
    fn collector_captures_tracy_frame_mark_events() {
        let _guard = TEST_LOGS_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        clear_logs();
        let subscriber = tracing_subscriber::Registry::default().with(LogCollectorLayer);

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(message = "finished frame", tracy.frame_mark = true);
        });

        let logs = log_snapshots();
        assert_eq!(logs.len(), 1);
        assert!(logs[0].is_tracy_frame_mark);
    }

    #[test]
    fn snapshot_delta_classifies_frame_mark_only_updates() {
        let _guard = TEST_LOGS_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        clear_logs();
        let previous = live_tracing_snapshot_revision();
        let subscriber = tracing_subscriber::Registry::default().with(LogCollectorLayer);

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(message = "finished frame", tracy.frame_mark = true);
        });

        assert_eq!(
            live_tracing_snapshot_delta_since(Some(previous)),
            LiveTracingSnapshotDelta::FrameMarksOnly
        );
    }

    #[test]
    fn snapshot_delta_classifies_regular_logs_as_other_updates() {
        let _guard = TEST_LOGS_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        clear_logs();
        let previous = live_tracing_snapshot_revision();
        let subscriber = tracing_subscriber::Registry::default().with(LogCollectorLayer);

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!("new live record");
        });

        assert_eq!(
            live_tracing_snapshot_delta_since(Some(previous)),
            LiveTracingSnapshotDelta::Other
        );
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

    #[test]
    fn collector_projects_non_app_scene_window_spans_as_open_timeline_spans() {
        let _guard = TEST_LOGS_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        clear_logs();
        let subscriber = tracing_subscriber::Registry::default().with(LogCollectorLayer);

        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!("scene_window", source_hwnd = 321_isize);
            let _entered = span.enter();

            let (dataset, _) = tracing_event_timeline_dataset();

            assert!(dataset.items().iter().any(|item| {
                matches!(item.kind(), crate::timeline::TimelineItemKind::Span(span) if span.end().is_none())
            }));
        });
    }
}
