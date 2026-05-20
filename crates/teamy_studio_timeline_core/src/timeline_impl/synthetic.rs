use arbitrary::Arbitrary;
use facet::Facet;

use super::dataset::{TimelineDataset, TimelineFieldInputValue, TimelineItemInput};
use super::time::TimelineInstantNs;

#[derive(Facet, Clone, Debug, PartialEq, Eq)]
// timeline[impl display.synthetic-data]
pub struct TimelineSyntheticConfig {
    seed: u64,
    job_count: usize,
    event_burst_count: usize,
    tiny_span_count: usize,
    object_event_count: usize,
    include_sparse_groups: bool,
    compact_after_generation: bool,
}

impl Default for TimelineSyntheticConfig {
    fn default() -> Self {
        Self {
            seed: 0x7469_6d65_6c69_6e65,
            job_count: 18,
            event_burst_count: 48,
            tiny_span_count: 64,
            object_event_count: 12,
            include_sparse_groups: true,
            compact_after_generation: true,
        }
    }
}

impl TimelineSyntheticConfig {
    #[must_use]
    pub const fn seed(&self) -> u64 {
        self.seed
    }

    #[must_use]
    pub const fn job_count(&self) -> usize {
        self.job_count
    }

    #[must_use]
    pub const fn event_burst_count(&self) -> usize {
        self.event_burst_count
    }

    #[must_use]
    pub const fn tiny_span_count(&self) -> usize {
        self.tiny_span_count
    }

    #[must_use]
    pub const fn object_event_count(&self) -> usize {
        self.object_event_count
    }

    #[must_use]
    pub const fn include_sparse_groups(&self) -> bool {
        self.include_sparse_groups
    }

    #[must_use]
    pub const fn compact_after_generation(&self) -> bool {
        self.compact_after_generation
    }

    #[must_use]
    pub const fn with_seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    #[must_use]
    pub const fn with_job_count(mut self, job_count: usize) -> Self {
        self.job_count = job_count;
        self
    }

    #[must_use]
    pub const fn with_event_burst_count(mut self, event_burst_count: usize) -> Self {
        self.event_burst_count = event_burst_count;
        self
    }

    #[must_use]
    pub const fn with_tiny_span_count(mut self, tiny_span_count: usize) -> Self {
        self.tiny_span_count = tiny_span_count;
        self
    }

    #[must_use]
    pub const fn with_object_event_count(mut self, object_event_count: usize) -> Self {
        self.object_event_count = object_event_count;
        self
    }

    #[must_use]
    pub const fn with_sparse_groups(mut self, include_sparse_groups: bool) -> Self {
        self.include_sparse_groups = include_sparse_groups;
        self
    }

    #[must_use]
    pub const fn with_compact_after_generation(mut self, compact_after_generation: bool) -> Self {
        self.compact_after_generation = compact_after_generation;
        self
    }
}

impl<'a> Arbitrary<'a> for TimelineSyntheticConfig {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        Ok(Self {
            seed: u64::arbitrary(u)?,
            job_count: u.int_in_range(0_usize..=32)?,
            event_burst_count: u.int_in_range(0_usize..=128)?,
            tiny_span_count: u.int_in_range(0_usize..=128)?,
            object_event_count: u.int_in_range(0_usize..=32)?,
            include_sparse_groups: bool::arbitrary(u)?,
            compact_after_generation: bool::arbitrary(u)?,
        })
    }
}

#[derive(Clone, Copy, Debug)]
struct SyntheticRng {
    state: u64,
}

impl SyntheticRng {
    const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.state
    }

    fn next_i64_range(&mut self, inclusive_max: i64) -> i64 {
        let modulo = u64::try_from(inclusive_max.saturating_add(1)).unwrap_or(1);
        i64::try_from(self.next_u64() % modulo).unwrap_or(0)
    }

    fn choose<'a>(&mut self, values: &'a [&'a str]) -> &'a str {
        let index = usize::try_from(self.next_u64()).unwrap_or(0) % values.len();
        values[index]
    }
}

/// # Errors
///
/// Returns an error if an internally generated closed span violates the timeline range invariant.
// timeline[impl display.synthetic-data]
pub fn generate_synthetic_timeline_dataset(
    config: &TimelineSyntheticConfig,
) -> eyre::Result<TimelineDataset> {
    let mut dataset = TimelineDataset::new();
    let mut rng = SyntheticRng::new(config.seed());

    add_job_spans(&mut dataset, &mut rng, config)?;
    add_dense_tiny_spans(&mut dataset, &mut rng, config)?;
    add_event_burst(&mut dataset, &mut rng, config);
    add_object_events(&mut dataset, &mut rng, config);

    if config.compact_after_generation() {
        dataset.compact();
    }

    Ok(dataset)
}

fn add_job_spans(
    dataset: &mut TimelineDataset,
    rng: &mut SyntheticRng,
    config: &TimelineSyntheticConfig,
) -> eyre::Result<()> {
    for index in 0..config.job_count() {
        let start = 1_000_000_i64
            .saturating_mul(i64::try_from(index).unwrap_or(i64::MAX))
            .saturating_add(rng.next_i64_range(250_000));
        let duration = 750_000_i64
            .saturating_add(rng.next_i64_range(4_000_000))
            .saturating_add(
                i64::try_from(index % 7)
                    .unwrap_or(0)
                    .saturating_mul(150_000),
            );
        let group_key = if config.include_sparse_groups() {
            format!("job-{}", index.saturating_mul(103).saturating_add(1))
        } else {
            format!("job-{index}")
        };
        let source_key = format!("synthetic/worker-{}", index % 4);
        let label = rng.choose(&[
            "Transcribe clip",
            "Import capture",
            "Render waveform",
            "Index project",
            "Upload artifact",
        ]);
        let object_id = 10_000_u64.saturating_add(u64::try_from(index).unwrap_or(u64::MAX));
        let input = TimelineItemInput::new(label)
            .with_source_key(source_key)
            .with_group_key(group_key)
            .with_field(
                "synthetic.kind",
                TimelineFieldInputValue::String("job-span".into()),
            )
            .with_field(
                "job.index",
                TimelineFieldInputValue::U64(u64::try_from(index).unwrap_or(u64::MAX)),
            )
            .with_field(
                "work.units",
                TimelineFieldInputValue::U64(rng.next_u64() % 1_000),
            )
            .with_field("object.id", TimelineFieldInputValue::U64(object_id))
            .with_field(
                "object.type_key",
                TimelineFieldInputValue::String("synthetic.job".into()),
            )
            .with_object_ref(object_id, "synthetic.job");
        let end = if index % 5 == 0 {
            None
        } else {
            Some(TimelineInstantNs::new(start.saturating_add(duration)))
        };
        dataset.push_span(input, TimelineInstantNs::new(start), end)?;
    }
    Ok(())
}

fn add_dense_tiny_spans(
    dataset: &mut TimelineDataset,
    rng: &mut SyntheticRng,
    config: &TimelineSyntheticConfig,
) -> eyre::Result<()> {
    let base = 50_000_000_i64;
    for index in 0..config.tiny_span_count() {
        let start = base
            .saturating_add(i64::try_from(index).unwrap_or(i64::MAX).saturating_mul(20))
            .saturating_add(rng.next_i64_range(3));
        let duration = 1_i64.saturating_add(rng.next_i64_range(5));
        let input = TimelineItemInput::new("Tiny profiler zone")
            .with_source_key("synthetic/dense-spans")
            .with_group_key("dense-spans")
            .with_field(
                "synthetic.kind",
                TimelineFieldInputValue::String("tiny-span".into()),
            )
            .with_field(
                "zone.index",
                TimelineFieldInputValue::U64(u64::try_from(index).unwrap_or(u64::MAX)),
            );
        dataset.push_span(
            input,
            TimelineInstantNs::new(start),
            Some(TimelineInstantNs::new(start.saturating_add(duration))),
        )?;
    }
    Ok(())
}

fn add_event_burst(
    dataset: &mut TimelineDataset,
    rng: &mut SyntheticRng,
    config: &TimelineSyntheticConfig,
) {
    let base = 75_000_000_i64;
    for index in 0..config.event_burst_count() {
        let at = base
            .saturating_add(i64::try_from(index).unwrap_or(i64::MAX).saturating_mul(3))
            .saturating_add(rng.next_i64_range(1));
        let level = rng.choose(&["info", "warn", "debug", "trace"]);
        let input = TimelineItemInput::new("Synthetic log event")
            .with_source_key("synthetic/events")
            .with_group_key("event-burst")
            .with_field(
                "synthetic.kind",
                TimelineFieldInputValue::String("event".into()),
            )
            .with_field("level", TimelineFieldInputValue::String(level.into()))
            .with_field(
                "event.index",
                TimelineFieldInputValue::U64(u64::try_from(index).unwrap_or(u64::MAX)),
            );
        dataset.push_event(input, TimelineInstantNs::new(at));
    }
}

fn add_object_events(
    dataset: &mut TimelineDataset,
    rng: &mut SyntheticRng,
    config: &TimelineSyntheticConfig,
) {
    // timeline[impl display.object-refs]
    let base = 100_000_000_i64;
    for index in 0..config.object_event_count() {
        let object_id = 20_000_u64.saturating_add(u64::try_from(index).unwrap_or(u64::MAX));
        let type_key = rng.choose(&[
            "synthetic.audio-buffer",
            "synthetic.transcript-chunk",
            "synthetic.render-snapshot",
        ]);
        let at = base.saturating_add(
            i64::try_from(index)
                .unwrap_or(i64::MAX)
                .saturating_mul(250_000),
        );
        let input = TimelineItemInput::new("Object observation")
            .with_source_key("synthetic/objects")
            .with_group_key(type_key)
            .with_field(
                "synthetic.kind",
                TimelineFieldInputValue::String("object".into()),
            )
            .with_field("object.id", TimelineFieldInputValue::U64(object_id))
            .with_field(
                "object.type_key",
                TimelineFieldInputValue::String(type_key.into()),
            )
            .with_field(
                "byte_count",
                TimelineFieldInputValue::U64(4_096_u64.saturating_add(rng.next_u64() % 65_536)),
            )
            .with_object_ref(object_id, type_key);
        dataset.push_event(input, TimelineInstantNs::new(at));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::{
        TimelineGroupingMode, TimelineItemKind, TimelineRenderItem, TimelineViewportQuery,
    };

    #[test]
    // timeline[verify display.synthetic-data]
    fn default_synthetic_dataset_is_compacted_and_renderable() {
        let dataset = generate_synthetic_timeline_dataset(&TimelineSyntheticConfig::default())
            .expect("synthetic dataset");
        let query = TimelineViewportQuery::try_new(
            TimelineInstantNs::new(0),
            TimelineInstantNs::new(125_000_000),
            TimelineInstantNs::new(125_000_000),
            1_200,
        )
        .expect("query")
        .with_minimum_visible_pixels(2);

        let plan = dataset.render_plan(&query);

        assert_eq!(dataset.pending_write_count(), 0);
        assert!(plan.rows().len() >= 4);
        assert!(!plan.items().is_empty());
    }

    #[test]
    // timeline[verify display.synthetic-data]
    // timeline[verify display.object-refs]
    fn synthetic_dataset_contains_open_spans_dense_items_sparse_groups_and_object_refs() {
        let dataset = generate_synthetic_timeline_dataset(&TimelineSyntheticConfig::default())
            .expect("synthetic dataset");

        assert!(dataset.items().iter().any(|item| matches!(
            item.kind(),
            TimelineItemKind::Span(span) if span.is_open()
        )));
        assert!(
            dataset
                .items()
                .iter()
                .any(|item| { dataset.resolve_string(item.group_key()) == Some("dense-spans") })
        );
        assert!(
            dataset
                .items()
                .iter()
                .any(|item| { dataset.resolve_string(item.group_key()) == Some("job-207") })
        );
        assert!(
            dataset
                .items()
                .iter()
                .any(|item| !item.object_refs().is_empty())
        );
    }

    #[test]
    // timeline[verify display.synthetic-data]
    // timeline[verify display.query-folding]
    fn synthetic_density_folds_into_clusters_when_zoomed_out() {
        let dataset = generate_synthetic_timeline_dataset(
            &TimelineSyntheticConfig::default()
                .with_job_count(0)
                .with_object_event_count(0),
        )
        .expect("synthetic dataset");
        let query = TimelineViewportQuery::try_new(
            TimelineInstantNs::new(49_000_000),
            TimelineInstantNs::new(76_000_000),
            TimelineInstantNs::new(76_000_000),
            300,
        )
        .expect("query")
        .with_grouping_mode(TimelineGroupingMode::GroupKey)
        .with_minimum_visible_pixels(2);

        let plan = dataset.render_plan(&query);

        assert!(
            plan.items()
                .iter()
                .any(|item| matches!(item, TimelineRenderItem::FoldedSpanCluster(_)))
        );
        assert!(
            plan.items()
                .iter()
                .any(|item| matches!(item, TimelineRenderItem::FoldedEventCluster(_)))
        );
    }

    #[test]
    // timeline[verify display.synthetic-data]
    fn arbitrary_synthetic_configs_generate_valid_renderable_datasets() {
        for seed in 0_u8..=u8::MAX {
            let bytes = [seed; 64];
            let mut unstructured = arbitrary::Unstructured::new(&bytes);
            let Ok(config) = TimelineSyntheticConfig::arbitrary(&mut unstructured) else {
                continue;
            };
            let mut dataset = generate_synthetic_timeline_dataset(&config).expect("dataset");
            dataset.compact();
            let query = TimelineViewportQuery::try_new(
                TimelineInstantNs::new(0),
                TimelineInstantNs::new(150_000_000),
                TimelineInstantNs::new(150_000_000),
                800,
            )
            .expect("query");

            let plan = dataset.render_plan(&query);

            assert_eq!(plan.pending_write_count(), 0);
        }
    }
}
