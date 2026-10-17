//! Scripted native-adapter input. The native focus/capture side effects and
//! reentrant callbacks are real; pointer samples enter through a test-only queue.
use super::*;
use baseview::{MouseButton, MouseEvent, dpi::PhysicalPosition};
use keyboard_types::Modifiers;

pub(super) fn enabled() -> bool {
    std::env::args().any(|arg| arg == "--test-input")
}

#[derive(Default)]
pub(super) struct Probe {
    text: String,
    pub clicks: usize,
    value: f64,
}
impl Probe {
    pub fn build(&mut self, ui: &mut Ui) -> El {
        col([
            mui::widgets::text_input(ui, "search", &mut self.text).el,
            block(160., 36.).id("row").focusable(),
            knob(ui, "drag", "Drag", &mut self.value, 0.0..=1.0)
                .el
                .into(),
        ])
        .size(320., 240.)
        .fill(Color::srgb(0., 1., 0.))
    }
}

#[derive(Default)]
pub(super) struct Script {
    step: usize,
    ready: Option<Instant>,
    clicks: usize,
    value: f64,
}

fn pointer(requests: &Requests, point: mui::prelude::Point) -> Result<(), String> {
    requests.test_event(Event::Mouse(MouseEvent::CursorMoved {
        position: PhysicalPosition::new(point.x, point.y),
        modifiers: Modifiers::empty(),
    }))
}
fn button(requests: &Requests, down: bool) -> Result<(), String> {
    let button = MouseButton::Left;
    let modifiers = Modifiers::empty();
    requests.test_event(Event::Mouse(if down {
        MouseEvent::ButtonPressed { button, modifiers }
    } else {
        MouseEvent::ButtonReleased { button, modifiers }
    }))
}

pub(super) fn advance(state: &mut State) -> Result<bool, String> {
    // Give the native pump time to apply output capture before posting the
    // next edge. This is input ordering, independent of rendering cadence.
    if state
        .input
        .ready
        .is_some_and(|at| at.elapsed() < Duration::from_millis(60))
    {
        return Ok(false);
    }
    let mut shared = state.shared.lock().map_err(|e| e.to_string())?;
    let center = |ui: &Ui, key: &str| -> Result<Point, String> {
        let frame = ui
            .scene()
            .and_then(|s| s.surface(key))
            .ok_or_else(|| format!("missing input surface {key}"))?
            .frame;
        Ok(Point::new(
            frame.x + frame.size.width / 2.,
            frame.y + frame.size.height / 2.,
        ))
    };
    match state.input.step {
        0 => {
            state.input.clicks = shared.view.input.clicks;
            state.input.value = shared.view.input.value;
            state.requests.focus();
            pointer(&state.requests, center(&shared.ui, "search")?)?;
            button(&state.requests, true)?;
        }
        1 | 6 => {
            button(&state.requests, false)?;
        }
        2 => {
            if !shared.ui.focus_is_text() {
                return Ok(false);
            }
            pointer(&state.requests, center(&shared.ui, "row")?)?;
            button(&state.requests, true)?;
        }
        3 => {
            if !shared.ui.get("row").held || shared.ui.focus_is_text() {
                return Ok(false);
            }
            button(&state.requests, false)?;
        }
        4 => {
            if shared.view.input.clicks == state.input.clicks
                && state
                    .input
                    .ready
                    .is_some_and(|at| at.elapsed() < Duration::from_secs(3))
            {
                return Ok(false);
            }
            if shared.view.input.clicks != state.input.clicks + 1 {
                return Err(
                    "dropdown row did not activate on its first click after text capture handoff"
                        .into(),
                );
            }
            pointer(&state.requests, center(&shared.ui, "drag")?)?;
            button(&state.requests, true)?;
        }
        5 => {
            let point = center(&shared.ui, "drag")?;
            for dy in [4., 12., 24.] {
                pointer(&state.requests, Point::new(point.x, point.y - dy))?;
            }
        }
        7 => {
            if shared.view.input.value == state.input.value {
                if state
                    .input
                    .ready
                    .is_some_and(|at| at.elapsed() < Duration::from_secs(3))
                {
                    return Ok(false);
                }
                return Err("native adapter drag did not change the control".into());
            }
            shared.ui.focus("search");
            state.requests.focus();
            state.requests.redraw();
        }
        8 => {
            if !shared.ui.focus_is_text() {
                return Ok(false);
            }
            eprintln!(
                "PASS native focus/capture handoff, dropdown first-click and drag round={}",
                state.round + 1
            );
            return Ok(true);
        }
        _ => return Err("invalid native input phase".into()),
    }
    state.input.step += 1;
    state.input.ready = Some(Instant::now());
    Ok(false)
}
