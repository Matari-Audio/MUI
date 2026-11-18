//! Opt-in, per-window CPU instrumentation. No locks, background workers or audio hooks.
//!
//! `PresentCall` measures the native present function returning; it does **not**
//! measure scanout, GPU completion, or input-to-photon latency. `Resolve` includes
//! Ui input dispatch, layout, scene resolution and retained-cache work together.
//! Percentiles describe the most recent bounded sample window, not all-time tails.
use std::collections::VecDeque;
use std::io::{self, Write};
use std::time::{Duration, Instant};

/// An observable CPU interval. Backends record only phases they can observe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Phase {
    InputQueueWait,
    ViewBuild,
    Resolve,
    Advance,
    Encode,
    Submit,
    PresentCall,
    BackendDraw,
    NativeWake,
    /// Oldest input arrival in a batch to the matching present call returning.
    InputToPresentCall,
}
impl Phase {
    pub const ALL: [Self; 10] = [
        Self::InputQueueWait,
        Self::ViewBuild,
        Self::Resolve,
        Self::Advance,
        Self::Encode,
        Self::Submit,
        Self::PresentCall,
        Self::BackendDraw,
        Self::NativeWake,
        Self::InputToPresentCall,
    ];
}

/// Cumulative counts; layout fields count work, not inferred cache misses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Counter {
    InputsEnqueued,
    InputsDispatched,
    HoverCoalesced,
    DragCoalesced,
    Invalidations,
    ModelChanges,
    Resizes,
    Frames,
    IdleSkips,
    InertHoverSkips,
    ThrottledSkips,
    MinimizedSkips,
    LayoutFailures,
    ClockRegressions,
    ValidatedNodes,
    MeasuredNodes,
    MeasureHits,
    ArrangedNodes,
    ReboundNodes,
    WeldHits,
    WeldMisses,
    NativeWakes,
    PresentCalls,
}
impl Counter {
    pub const ALL: [Self; 23] = [
        Self::InputsEnqueued,
        Self::InputsDispatched,
        Self::HoverCoalesced,
        Self::DragCoalesced,
        Self::Invalidations,
        Self::ModelChanges,
        Self::Resizes,
        Self::Frames,
        Self::IdleSkips,
        Self::InertHoverSkips,
        Self::ThrottledSkips,
        Self::MinimizedSkips,
        Self::LayoutFailures,
        Self::ClockRegressions,
        Self::ValidatedNodes,
        Self::MeasuredNodes,
        Self::MeasureHits,
        Self::ArrangedNodes,
        Self::ReboundNodes,
        Self::WeldHits,
        Self::WeldMisses,
        Self::NativeWakes,
        Self::PresentCalls,
    ];
}

#[derive(Clone, Copy, Debug)]
pub struct ProfileConfig {
    /// Recent samples retained per phase, clamped to 1..=4096.
    pub capacity: usize,
}
impl Default for ProfileConfig {
    fn default() -> Self {
        Self { capacity: 512 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub sequence: u64,
    pub elapsed: Duration,
    pub duration: Duration,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Percentiles {
    pub retained: usize,
    pub total: u64,
    pub p50: Duration,
    pub p95: Duration,
    pub p99: Duration,
}

/// Own one on the UI thread of each window. Recording uses preallocated rings.
/// Export and percentile sorting are explicit developer operations off hot paths.
pub struct Profiler {
    origin: Instant,
    capacity: usize,
    samples: [VecDeque<Sample>; 10],
    totals: [u64; 10],
    counts: [u64; 23],
    sequence: u64,
    /// Time per phase since [`Profiler::take_frame`] last emptied it.
    frame: [Duration; 10],
    oldest_input: Option<Instant>,
    present_input: Option<Instant>,
}
impl Profiler {
    pub fn new(config: ProfileConfig) -> Self {
        let capacity = config.capacity.clamp(1, 4096);
        Self {
            origin: Instant::now(),
            capacity,
            samples: std::array::from_fn(|_| VecDeque::with_capacity(capacity)),
            totals: [0; 10],
            counts: [0; 23],
            sequence: 0,
            frame: [Duration::ZERO; 10],
            oldest_input: None,
            present_input: None,
        }
    }
    pub fn count(&mut self, counter: Counter, amount: u64) {
        let count = &mut self.counts[counter as usize];
        *count = count.saturating_add(amount);
    }
    pub fn counter(&self, counter: Counter) -> u64 {
        self.counts[counter as usize]
    }
    pub fn record_since(&mut self, phase: Phase, start: Instant) {
        self.record_between(phase, start, Instant::now());
    }
    /// Supplied monotonic timestamps must belong to the same clock domain.
    /// Reversed clocks are counted and omitted rather than reported as zero.
    pub fn record_between(&mut self, phase: Phase, start: Instant, end: Instant) {
        let Some(duration) = end.checked_duration_since(start) else {
            self.count(Counter::ClockRegressions, 1);
            return;
        };
        self.record_at(phase, duration, end);
    }
    pub fn record_duration(&mut self, phase: Phase, duration: Duration) {
        self.record_at(phase, duration, Instant::now());
    }
    fn record_at(&mut self, phase: Phase, duration: Duration, end: Instant) {
        if phase == Phase::PresentCall {
            self.count(Counter::PresentCalls, 1);
        }
        if phase == Phase::NativeWake {
            self.count(Counter::NativeWakes, 1);
        }
        self.sequence = self.sequence.saturating_add(1);
        let i = phase as usize;
        self.totals[i] = self.totals[i].saturating_add(1);
        self.frame[i] = self.frame[i].saturating_add(duration);
        let ring = &mut self.samples[i];
        if ring.len() == self.capacity {
            ring.pop_front();
        }
        ring.push_back(Sample {
            sequence: self.sequence,
            elapsed: end.saturating_duration_since(self.origin),
            duration,
        });
        if phase == Phase::PresentCall
            && let Some(input) = self.present_input.take()
        {
            self.record_between(Phase::InputToPresentCall, input, end);
        }
    }
    /// Time spent in each phase (indexed by [`Phase`]) since the last call:
    /// one frame's worth when a host calls it once per frame.
    pub fn take_frame(&mut self) -> [Duration; 10] {
        std::mem::take(&mut self.frame)
    }
    pub fn samples(&self, phase: Phase) -> impl Iterator<Item = &Sample> {
        self.samples[phase as usize].iter()
    }
    pub fn percentiles(&self, phase: Phase) -> Percentiles {
        let mut sorted: Vec<_> = self.samples(phase).map(|s| s.duration).collect();
        sorted.sort_unstable();
        let percentile = |p: usize| {
            sorted
                .get((sorted.len() * p).div_ceil(100).saturating_sub(1))
                .copied()
                .unwrap_or_default()
        };
        Percentiles {
            retained: sorted.len(),
            total: self.totals[phase as usize],
            p50: percentile(50),
            p95: percentile(95),
            p99: percentile(99),
        }
    }
    /// CSV records use stable Rust variant names and integer nanoseconds.
    /// Call outside frame/audio callbacks; this may perform blocking I/O.
    pub fn write_csv(&self, mut out: impl Write) -> io::Result<()> {
        writeln!(out, "kind,name,sequence,elapsed_ns,duration_ns,count")?;
        for phase in Phase::ALL {
            for sample in self.samples(phase) {
                writeln!(
                    out,
                    "sample,{phase:?},{},{},{},",
                    sample.sequence,
                    sample.elapsed.as_nanos(),
                    sample.duration.as_nanos()
                )?;
            }
        }
        for counter in Counter::ALL {
            writeln!(out, "counter,{counter:?},,,,{}", self.counter(counter))?;
        }
        Ok(())
    }
    pub(crate) fn input_enqueued(&mut self) {
        self.count(Counter::InputsEnqueued, 1);
        self.oldest_input.get_or_insert_with(Instant::now);
    }
    pub(crate) fn dispatch_batch(&mut self) {
        if let Some(input) = self.oldest_input.take() {
            self.record_since(Phase::InputQueueWait, input);
            self.present_input = Some(
                self.present_input
                    .map_or(input, |pending| pending.min(input)),
            );
        }
    }
    pub(crate) fn discard_inputs(&mut self) {
        self.oldest_input = None;
    }
    /// The backend confirms the newly resolved scene has no visual change.
    /// Clear its input marker so an unrelated later present cannot claim latency.
    /// Keep the marker for a skipped/deferred presentation that will be retried.
    pub fn discard_pending_presentation(&mut self) {
        self.present_input = None;
    }

    pub(crate) fn finish_inputs(&mut self) {
        self.oldest_input = None;
        self.discard_pending_presentation();
    }
}

/// How a host drew one frame, for [`FrameLog::write`].
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameRecord<'a> {
    /// Device pixels.
    pub size: (u32, u32),
    /// `true` on the GPU presenter, `false` on CPU frames.
    pub gpu: bool,
    /// Why the host is on the presenter it is on ("GPU ready", "CPU startup", ...).
    pub reason: &'a str,
    /// The frame reached the screen; `false` when nothing had changed or the
    /// present was skipped (a lost surface, a resize in flight).
    pub presented: bool,
    /// The GPU pass refused the frame and it was rasterised whole on the CPU.
    pub cpu_fallback: Option<&'a dyn std::fmt::Debug>,
    /// Entries rasterised alone on the CPU, the rest staying on the GPU.
    pub cpu_patches: usize,
    /// Pixels the GPU pass drew: the whole window or the box around damage.
    pub rendered_pixels: u64,
}

/// One JSON line per frame in the diagnostics directory, for any host to
/// write when `MUI_FRAME_LOG` is set, so a slow installed plugin can be
/// read back frame by frame: each phase's time, the presenter, and why a
/// frame left the GPU. Opt-in; it writes on the UI thread.
pub struct FrameLog {
    file: std::fs::File,
    path: std::path::PathBuf,
    origin: Instant,
    written: u64,
}

impl FrameLog {
    /// ponytail: stops at 64 MiB (~200k frames); rotate if longer sessions matter.
    const MAX_BYTES: u64 = 64 << 20;

    /// A log when `MUI_FRAME_LOG` is set to anything but `0`, else `None`.
    pub fn from_env() -> Option<Self> {
        let on = std::env::var_os("MUI_FRAME_LOG").is_some_and(|v| !v.is_empty() && v != "0");
        on.then(|| Self::create(&crate::diagnostics::directory()))?
            .ok()
    }

    /// A new log in `directory`, named for this process and window.
    pub fn create(directory: &std::path::Path) -> io::Result<Self> {
        std::fs::create_dir_all(directory)?;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = directory.join(format!("frames-{}-{stamp:x}.jsonl", std::process::id()));
        let file = std::fs::File::create(&path)?;
        Ok(Self {
            file,
            path,
            origin: Instant::now(),
            written: 0,
        })
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// Append `frame` with the phase times `profiler` gathered since the
    /// last write. A failed write stops the log; it never fails the frame.
    pub fn write(&mut self, profiler: &mut Profiler, frame: FrameRecord) {
        use std::fmt::Write as _;
        let phases = profiler.take_frame();
        if self.written >= Self::MAX_BYTES {
            return;
        }
        let mut line = format!(
            r#"{{"t_us":{},"w":{},"h":{},"gpu":{},"reason":{:?},"presented":{},"cpu_patches":{},"rendered_px":{}"#,
            self.origin.elapsed().as_micros(),
            frame.size.0,
            frame.size.1,
            frame.gpu,
            frame.reason,
            frame.presented,
            frame.cpu_patches,
            frame.rendered_pixels,
        );
        if let Some(reason) = frame.cpu_fallback {
            let _ = write!(line, r#","cpu_fallback":"{reason:?}""#);
        }
        line.push_str(r#","us":{"#);
        let mut first = true;
        for phase in Phase::ALL {
            let time = phases[phase as usize];
            if !time.is_zero() {
                let comma = if std::mem::take(&mut first) { "" } else { "," };
                let _ = write!(line, r#"{comma}"{phase:?}":{}"#, time.as_micros());
            }
        }
        line.push_str("}}\n");
        match self.file.write_all(line.as_bytes()) {
            Ok(()) => self.written += line.len() as u64,
            Err(_) => self.written = Self::MAX_BYTES,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn samples_are_bounded_and_percentiles_are_recent() {
        let mut profiler = Profiler::new(ProfileConfig { capacity: 100 });
        for n in 1..=200 {
            profiler.record_duration(Phase::ViewBuild, Duration::from_nanos(n));
        }
        let stats = profiler.percentiles(Phase::ViewBuild);
        assert_eq!((stats.retained, stats.total), (100, 200));
        assert_eq!(
            (
                stats.p50.as_nanos(),
                stats.p95.as_nanos(),
                stats.p99.as_nanos()
            ),
            (150, 195, 199)
        );
        let now = Instant::now();
        profiler.record_between(Phase::Encode, now, now - Duration::from_secs(1));
        assert_eq!(profiler.counter(Counter::ClockRegressions), 1);
        assert_eq!(profiler.percentiles(Phase::Encode).total, 0);
        let mut csv = Vec::new();
        profiler.write_csv(&mut csv).unwrap();
        assert!(
            String::from_utf8(csv)
                .unwrap()
                .contains("counter,ClockRegressions,,,,1")
        );
    }
    #[test]
    fn presentation_latency_is_only_observed_when_present_returns() {
        let mut profiler = Profiler::new(ProfileConfig::default());
        profiler.input_enqueued();
        profiler.dispatch_batch();
        assert_eq!(profiler.percentiles(Phase::InputToPresentCall).total, 0);
        profiler.record_duration(Phase::PresentCall, Duration::ZERO);
        assert_eq!(profiler.percentiles(Phase::InputToPresentCall).total, 1);
        profiler.record_duration(Phase::PresentCall, Duration::ZERO);
        assert_eq!(profiler.percentiles(Phase::InputToPresentCall).total, 1);
    }
    #[test]
    fn unchanged_frames_do_not_link_input_to_an_unrelated_present() {
        let mut profile = Profiler::new(ProfileConfig::default());
        profile.input_enqueued();
        profile.dispatch_batch();
        // Backend Frame::Current: this input produces no visual change.
        profile.discard_pending_presentation();
        profile.record_duration(Phase::PresentCall, Duration::ZERO);
        assert_eq!(profile.percentiles(Phase::InputToPresentCall).total, 0);
        assert_eq!(profile.counter(Counter::PresentCalls), 1);
    }

    #[test]
    fn deferred_presents_preserve_earliest_input_and_close_clears_markers() {
        let mut profile = Profiler::new(ProfileConfig::default());
        let now = Instant::now();
        let first = now - Duration::from_millis(20);
        profile.oldest_input = Some(first);
        profile.dispatch_batch();
        // Backend Frame::Skipped: no present occurred, and another input arrives.
        profile.oldest_input = Some(now - Duration::from_millis(10));
        profile.dispatch_batch();
        assert_eq!(profile.present_input, Some(first));
        profile.record_between(Phase::PresentCall, now, now + Duration::from_millis(5));
        assert_eq!(
            profile.percentiles(Phase::InputToPresentCall).p50,
            Duration::from_millis(25)
        );
        profile.input_enqueued();
        profile.dispatch_batch();
        profile.input_enqueued();
        profile.finish_inputs();
        assert_eq!(profile.oldest_input, None);
        assert_eq!(profile.present_input, None);
        profile.record_duration(Phase::PresentCall, Duration::ZERO);
        assert_eq!(profile.percentiles(Phase::InputToPresentCall).total, 1);
    }

    #[test]
    fn the_frame_log_writes_a_line_of_phase_times_per_frame() {
        let dir = std::env::temp_dir().join(format!("mui-frame-log-{}", std::process::id()));
        let mut log = FrameLog::create(&dir).unwrap();
        let mut profiler = Profiler::new(ProfileConfig::default());
        profiler.record_duration(Phase::Resolve, Duration::from_micros(1500));
        profiler.record_duration(Phase::Resolve, Duration::from_micros(500));
        log.write(
            &mut profiler,
            FrameRecord {
                size: (800, 600),
                reason: "CPU startup",
                presented: true,
                cpu_fallback: Some(&Phase::Encode),
                ..FrameRecord::default()
            },
        );
        log.write(&mut profiler, FrameRecord::default());
        let text = std::fs::read_to_string(log.path()).unwrap();
        let lines: Vec<_> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(
            lines[0].contains(r#""us":{"Resolve":2000}"#),
            "{}",
            lines[0]
        );
        assert!(
            lines[0].contains(r#""reason":"CPU startup""#),
            "{}",
            lines[0]
        );
        assert!(
            lines[0].contains(r#""cpu_fallback":"Encode""#),
            "{}",
            lines[0]
        );
        assert!(lines[1].ends_with(r#""us":{}}"#), "{}", lines[1]);
        let _ = std::fs::remove_dir_all(dir);
    }
}
