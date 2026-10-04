//! Matched CPU workload contract for MUI/native backend/GPUI comparisons.
//! cargo run -p mui --example profile_scenarios --release -- --frames 600 --output /tmp/mui-profile
//! Reports CPU driver phases only; no headless draw/present/scanout claim.
use mui::host::{Driver, Shared, View};
use mui::prelude::*;
use mui::profiling::{Counter, Phase, ProfileConfig};
use std::fs::File;
use std::time::Duration;

struct NoClipboard;
impl mui::Clipboard for NoClipboard {
    fn get(&mut self) -> Option<String> {
        None
    }
    fn set(&mut self, _: &str) {}
}
#[derive(Clone, Copy, Debug)]
enum Scenario {
    KnobMeter,
    List,
    Vectors,
    Idle,
    Resize,
    Reopen,
}
struct Panel {
    scenario: Scenario,
    value: f64,
    tick: usize,
    changed: bool,
}
impl View for Panel {
    fn changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }
    fn request_resize(&mut self, _: u32, _: u32) -> bool {
        false
    }
    fn build(&mut self, ui: &mut Ui, _: &Input) -> El {
        match self.scenario {
            Scenario::List => {
                col((0..1000).map(|i| text(format!("Track {i:04}")).h(24.).id(format!("row/{i}"))))
                    .w(600.)
                    .h(600.)
                    .scroll()
                    .id("list")
            }
            Scenario::Vectors => {
                let tick = self.tick;
                canvas(move |size| {
                    (0..64)
                        .map(|line| {
                            let points = (0..128).map(|i| {
                                Point::new(
                                    size.width * i as f64 / 127.,
                                    size.height
                                        * (0.5 + 0.45 * ((i + line + tick) as f64 * 0.1).sin()),
                                )
                            });
                            Draw::stroke(Path::polyline(points, false), Role::Primary, 1.)
                        })
                        .collect()
                })
                .size(600., 600.)
                .id("vectors")
            }
            Scenario::KnobMeter => {
                let knob = knob(ui, "knob", "Gain", &mut self.value, 0.0..=1.0)
                    .el
                    .into();
                let meter = meter(ui, "meter", self.value).w(300.);
                col([knob, meter]).gap(12.).w(600.)
            }
            Scenario::Idle | Scenario::Resize | Scenario::Reopen => col((0..24).map(|i| {
                text(format!("Channel {i:02}"))
                    .h(24.)
                    .id(format!("channel/{i}"))
            }))
            .w(600.),
        }
    }
}
fn driver() -> Driver {
    Driver::new((800, 800), 1., Box::new(NoClipboard))
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut frames = 600usize;
    let mut output = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--frames" => frames = args.next().ok_or("--frames needs a count")?.parse()?,
            "--output" => output = Some(args.next().ok_or("--output needs a path prefix")?),
            _ => return Err(format!("unknown argument: {arg}").into()),
        }
    }
    if frames == 0 {
        return Err("frames must be positive".into());
    }
    println!(
        "CPU-only contract: 800x800 physical, scale 1, 60Hz logical ticks, Hack font; 1000 list rows x24px; 64 vector paths x128 points; profile ring 512/phase. No GPU/present timing."
    );
    println!(
        "scenario,scenes_ready,build_samples,resolve_samples,build_p50_us,build_p95_us,build_p99_us,resolve_p50_us,resolve_p95_us,resolve_p99_us,idle_skips"
    );
    for scenario in [
        Scenario::KnobMeter,
        Scenario::List,
        Scenario::Vectors,
        Scenario::Idle,
        Scenario::Resize,
        Scenario::Reopen,
    ] {
        let mut shared = Shared {
            ui: Ui::default().font(Font::new(epaint_default_fonts::HACK_REGULAR)?),
            view: Panel {
                scenario,
                value: 0.5,
                tick: 0,
                changed: false,
            },
        };
        let mut driver = driver();
        let mut now = driver.last_frame();
        // Settle text/layout/springs before recording warm-workload samples.
        for _ in 0..120 {
            now += Duration::from_secs_f64(1. / 60.);
            driver.advance(&mut shared, now);
        }
        driver.enable_profiling(ProfileConfig::default());
        for tick in 0..frames {
            shared.view.tick = tick;
            now += Duration::from_secs_f64(1. / 60.);
            match scenario {
                Scenario::KnobMeter => {
                    shared.view.value = (tick % 120) as f64 / 119.;
                    shared.view.changed = true;
                    // Four native hover samples per tick, with edges each 30 ticks.
                    for sample in 0..4 {
                        driver.pointer_moved(Point::new(30. + sample as f64, 30.), Mods::default());
                    }
                    if tick % 30 == 0 {
                        driver.button(Button::Primary, true, Mods::default());
                    }
                    if tick % 30 == 15 {
                        driver.button(Button::Primary, false, Mods::default());
                    }
                }
                Scenario::List => {
                    driver.pointer_moved(Point::new(200., 200.), Mods::default());
                    driver.wheel(mui::host::Wheel::Pixels(0., -24.), Mods::default());
                }
                Scenario::Vectors => shared.view.changed = true,
                Scenario::Resize => driver.resized((700 + (tick % 100) as u32, 800), 1.),
                Scenario::Reopen => {
                    // Reopen means a new retained Ui and a new Driver, rather than redraw.
                    // Move the profiling session across windows solely to aggregate this workload.
                    let profiler = driver.take_profiler();
                    driver.close(&mut shared);
                    shared.ui = Ui::default().font(Font::new(epaint_default_fonts::HACK_REGULAR)?);
                    driver = self::driver();
                    driver.set_profiler(profiler);
                    now = driver.last_frame() + Duration::from_secs_f64(1. / 60.);
                }
                Scenario::Idle => {}
            }
            std::hint::black_box(driver.advance(&mut shared, now));
        }
        let profile = driver.profiler().unwrap();
        let build = profile.percentiles(Phase::ViewBuild);
        let resolve = profile.percentiles(Phase::Resolve);
        let us = |d: Duration| d.as_secs_f64() * 1e6;
        println!(
            "{scenario:?},{},{},{},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{}",
            profile.counter(Counter::Frames),
            build.total,
            resolve.total,
            us(build.p50),
            us(build.p95),
            us(build.p99),
            us(resolve.p50),
            us(resolve.p95),
            us(resolve.p99),
            profile.counter(Counter::IdleSkips)
        );
        if let Some(prefix) = &output {
            profile.write_csv(File::create(format!("{prefix}-{scenario:?}.csv"))?)?;
        }
    }
    Ok(())
}
