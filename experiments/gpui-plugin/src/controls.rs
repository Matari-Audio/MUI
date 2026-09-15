//! Shared GPUI key bindings; parameter state and host gestures stay with each component.
use gpui::{prelude::*, *};
use std::rc::Rc;

actions!(
    mui_controls,
    [
        LargerType,
        SmallerType,
        NextFocus,
        PreviousFocus,
        Cancel,
        Increase,
        Decrease,
        FineIncrease,
        FineDecrease,
        Connect,
        PreviousRoute,
        NextRoute,
        IncreaseDepth,
        DecreaseDepth,
        RemoveRoute
    ]
);
struct Installed;
impl Global for Installed {}
pub fn init(cx: &mut App) {
    if cx.has_global::<Installed>() {
        return;
    }
    cx.set_global(Installed);
    cx.bind_keys([
        KeyBinding::new("+", LargerType, Some("MUIOscillator")),
        KeyBinding::new("=", LargerType, Some("MUIOscillator")),
        KeyBinding::new("-", SmallerType, Some("MUIOscillator")),
        KeyBinding::new("tab", NextFocus, Some("MUI")),
        KeyBinding::new("shift-tab", PreviousFocus, Some("MUI")),
        KeyBinding::new("escape", Cancel, Some("MUI")),
        KeyBinding::new("up", Increase, Some("MUIParameter")),
        KeyBinding::new("right", Increase, Some("MUIParameter")),
        KeyBinding::new("down", Decrease, Some("MUIParameter")),
        KeyBinding::new("left", Decrease, Some("MUIParameter")),
        KeyBinding::new("shift-up", FineIncrease, Some("MUIParameter")),
        KeyBinding::new("shift-right", FineIncrease, Some("MUIParameter")),
        KeyBinding::new("shift-down", FineDecrease, Some("MUIParameter")),
        KeyBinding::new("shift-left", FineDecrease, Some("MUIParameter")),
        KeyBinding::new("enter", Connect, Some("MUIRoute")),
        KeyBinding::new("alt-left", PreviousRoute, Some("MUIRoute")),
        KeyBinding::new("alt-right", NextRoute, Some("MUIRoute")),
        KeyBinding::new("alt-up", IncreaseDepth, Some("MUIRoute")),
        KeyBinding::new("alt-down", DecreaseDepth, Some("MUIRoute")),
        KeyBinding::new("alt-backspace", RemoveRoute, Some("MUIRoute")),
        KeyBinding::new("alt-delete", RemoveRoute, Some("MUIRoute")),
    ]);
}
pub fn navigation(el: Stateful<Div>) -> Stateful<Div> {
    el.key_context("MUI")
        .on_action(|_: &NextFocus, window, cx| window.focus_next(cx))
        .on_action(|_: &PreviousFocus, window, cx| window.focus_prev(cx))
}
pub fn parameter(
    el: Stateful<Div>,
    target: impl Into<Option<crate::modulation::Target>>,
    enabled: bool,
    change: impl Fn(&f32, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let change = Rc::new(change);
    let increase = change.clone();
    let decrease = change.clone();
    let fine = change.clone();
    el.role(Role::Slider)
        .when_some(target.into().filter(|_| enabled), |el, target| {
            crate::modulation::target_actions(el, target).child(crate::modulation::marker(
                crate::modulation::Anchor::Target(target),
                0.,
            ))
        })
        .focus_visible(|s| s.bg(rgba(0xffffff28)))
        .on_action(move |_: &Increase, w, cx| increase(&1., w, cx))
        .on_action(move |_: &Decrease, w, cx| decrease(&-1., w, cx))
        .on_action(move |_: &FineIncrease, w, cx| fine(&0.1, w, cx))
        .on_action(move |_: &FineDecrease, w, cx| change(&-0.1, w, cx))
}
