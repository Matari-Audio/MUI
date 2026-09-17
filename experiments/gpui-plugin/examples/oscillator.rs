use gpui::{prelude::*, *};
use mui_gpui_plugin_probe::oscillator::{GroupEvent, OscillatorGroup};
fn main() {
    if std::env::args().any(|a| a == "--export" || a == "--bench") {
        Application::with_platform(gpui_linux::current_platform(true)).run(|cx| {
            let group = cx.new(OscillatorGroup::new);
            std::fs::write(
                "docs/oscillator-group.svg",
                group.read(cx).svg(cx).expect("group SVG"),
            )
            .expect("write group SVG");
            let oscillator = cx.new(mui_gpui_plugin_probe::oscillator::Oscillator::new);
            std::fs::write(
                "docs/oscillator.svg",
                oscillator.read(cx).svg(960.).expect("oscillator SVG"),
            )
            .expect("write oscillator SVG");
            if std::env::args().any(|a| a == "--bench") {
                group.read(cx).benchmark(cx).expect("group benchmark");
                oscillator
                    .update(cx, |oscillator, _| oscillator.benchmark())
                    .expect("benchmark");
            }
            std::process::exit(0);
        });
        return;
    }
    if std::env::args().any(|a| a == "--kurv") {
        run_kurv();
        return;
    }
    Application::with_platform(gpui_linux::current_platform(false)).run(move |cx| {
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(960.), px(1080.)),
                    cx,
                ))),
                window_min_size: Some(size(px(760.), px(376.))),
                titlebar: Some(TitlebarOptions {
                    title: Some("MUI · Oscillator Group".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |_, cx| {
                let entity = cx.new(OscillatorGroup::new);
                if let Ok(path) = std::env::var("MUI_OSCILLATOR_SVG") {
                    std::fs::write(path, entity.read(cx).svg(cx).expect("group SVG"))
                        .expect("write group SVG");
                }
                cx.subscribe(&entity, |_, event: &GroupEvent, _| eprintln!("{event:?}"))
                    .detach();
                entity
            },
        )
        .expect("open oscillator group");
        cx.activate(true);
    });
}

fn run_kurv() {
    let width = std::env::var("MUI_CHECK_WIDTH")
        .map(|value| {
            value
                .parse::<f32>()
                .expect("MUI_CHECK_WIDTH must be a number")
        })
        .unwrap_or(1480.);
    assert!(
        width.is_finite() && width >= 1280.,
        "minimum width is 1280 logical pixels"
    );
    Application::with_platform(gpui_linux::current_platform(false)).run(move |cx| {
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(width), px(1040.)),
                    cx,
                ))),
                window_min_size: Some(size(px(1280.), px(600.))),
                titlebar: Some(TitlebarOptions {
                    title: Some("MUI · KURV Composition".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |window, cx| {
                let workspace = cx.new(mui_gpui_plugin_probe::kurv::KurvWorkspace::new);
                if std::env::args().any(|a| a == "--check-interactions") {
                    workspace.update(cx, |workspace, cx| workspace.check_interactions(window, cx));
                }
                workspace
            },
        )
        .expect("open KURV composition");
        cx.activate(true);
    });
}
