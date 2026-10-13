//! CPU scheduling profile: resize samples at 60 Hz, live data halfway between,
//! ordered IME input every tenth sample, and one final layout reconciliation.
//! No native presentation or GPU timing is claimed.
use mui::host::{Driver, Shared, View};
use mui::prelude::*;
use mui::profiling::{Phase, ProfileConfig};
use std::time::Duration;

struct Clipboard;
impl mui::Clipboard for Clipboard {
    fn get(&mut self) -> Option<String> {
        None
    }
    fn set(&mut self, _: &str) {}
}
struct Mixer {
    values: [f64; 64],
    changed: bool,
    builds: usize,
}
impl View for Mixer {
    fn changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }
    fn request_resize(&mut self, _: u32, _: u32) -> bool {
        false
    }
    fn build(&mut self, ui: &mut Ui, _: &Input) -> El {
        self.builds += 1;
        col(self.values.iter_mut().enumerate().map(|(i, value)| {
            row([
                text(format!("Channel {i:02}")).w(120.),
                knob(ui, format!("gain/{i}"), "Gain", value, 0.0..=1.0).into(),
                meter(ui, format!("meter/{i}"), *value).w(240.),
            ])
            .gap(8.)
            .id(format!("channel/{i}"))
        }))
        .size(600., 600.)
        .scroll()
        .id("mixer")
    }
}
fn main() {
    let mut shared = Shared {
        ui: Ui::default().font(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap()),
        view: Mixer {
            values: [0.5; 64],
            changed: false,
            builds: 0,
        },
    };
    let mut driver = Driver::new((800, 800), 1., Box::new(Clipboard));
    let mut now = driver.last_frame();
    for _ in 0..120 {
        now += Duration::from_millis(16);
        driver.advance(&mut shared, now);
    }
    driver.enable_profiling(ProfileConfig::default());
    let start = std::time::Instant::now();
    let mut resize_frames = 0;
    let mut live_frames = 0;
    let mut input_frames = 0;
    for sample in 0..200 {
        now += Duration::from_millis(16);
        driver.resized_at((780 + (sample % 20) as u32, 800), 1., now);
        resize_frames += usize::from(driver.advance(&mut shared, now));
        shared.view.values.fill((sample % 100) as f64 / 100.);
        shared.view.changed = true;
        live_frames += usize::from(driver.advance(&mut shared, now + Duration::from_millis(8)));
        if sample % 10 == 0 {
            driver.ime(mui::input::Ime::Commit("a".into()));
            input_frames +=
                usize::from(driver.advance(&mut shared, now + Duration::from_millis(9)));
        }
    }
    let final_frame = driver.advance(&mut shared, now + Duration::from_millis(35));
    let elapsed = start.elapsed();
    let profile = driver.take_profiler().unwrap();
    println!(
        "resize_frames={resize_frames} live_frames={live_frames} input_frames={input_frames} final_frame={final_frame} driver_cpu_ms={:.3} builds={} resolve_samples={} build_p50_us={:.3} build_p95_us={:.3}",
        elapsed.as_secs_f64() * 1000.,
        profile.percentiles(Phase::ViewBuild).total,
        profile.percentiles(Phase::Resolve).total,
        profile.percentiles(Phase::ViewBuild).p50.as_secs_f64() * 1_000_000.,
        profile.percentiles(Phase::ViewBuild).p95.as_secs_f64() * 1_000_000.
    );
    assert_eq!(resize_frames, 200, "all resize samples must present");
    assert_eq!(input_frames, 20, "input must bypass automatic-work gates");
    assert!(final_frame, "a final layout reconciliation must occur");
}
