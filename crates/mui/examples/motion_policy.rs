//! Headless policy and scheduling demo: `cargo run -p mui --example motion_policy`.
//! A native host forwards preference changes with `set_system_reduced_motion`;
//! redraw only when that setter returns true. This demo supplies that preference.
use mui::prelude::*;

fn card(wide: bool) -> El {
    block(if wide { 160. } else { 80. }, 40.)
        .fill(if wide { Role::Primary } else { Role::Field })
        .animate()
        .animate_layout()
        .id("card")
}

fn settle(ui: &mut Ui, wide: bool) -> usize {
    for frames in 1..=180 {
        let frame = ui
            .frame(card(wide), None, Input::default(), 1. / 60.)
            .expect("frame");
        if !frame.animating {
            assert_eq!(
                frame.scene.surface("card").unwrap().frame.size.width,
                if wide { 160. } else { 80. }
            );
            assert_eq!(frame.repaint_after, None);
            return frames;
        }
    }
    panic!("UI animation did not settle");
}

fn main() {
    let mut ui = Ui::default();
    settle(&mut ui, false);
    let full = settle(&mut ui, true);
    assert!(full > 1);
    assert!(ui.set_system_reduced_motion(true));
    assert_eq!(settle(&mut ui, false), 1);
    assert!(!ui.set_system_reduced_motion(true));
    assert!(ui.set_motion_policy(MotionPolicy::Full));
    let restored = settle(&mut ui, true);
    assert!(restored > 1);
    // Explicitly sampled media time is independent of UI accessibility policy.
    let media = Keys::new(0.).to(1., 1., Ease::Linear);
    assert!(ui.set_motion_policy(MotionPolicy::Reduced));
    assert_eq!(ui.play("entrance", &media), 1.);
    assert_eq!(media.at(0.25), 0.25);
    println!(
        "Full: {full} frames; reduced: 1 frame; restored: {restored} frames; idle has no wakeup."
    );
}
