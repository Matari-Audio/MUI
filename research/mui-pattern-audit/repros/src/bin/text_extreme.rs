//! Checks text size conversion and derived-metric validation. Supply a valid font path.
fn main() {
    let path = std::env::args_os()
        .nth(1)
        .expect("usage: text_extreme FONT.ttf");
    let font = std::fs::read(path).expect("read font");
    let ordinary = mui_text::text_run(&font, "A", 96.0, &[], 0.1).expect("ordinary text run");
    assert!(ordinary.advance.is_finite());
    assert!(ordinary.ascent.is_finite());
    assert!(ordinary.descent.is_finite());
    assert!(ordinary.line_height.is_finite());
    assert!(mui_text::glyph_path(&font, 'A', 96.0, &[], 0.1).is_ok());
    println!("PASS: ordinary text size produces finite metrics and a valid glyph path");

    let max_size = f32::MAX as f64;
    let max_glyph = mui_text::glyph_path(&font, 'A', max_size, &[], 0.1)
        .expect("f32::MAX is representable and keeps finite path commands");
    assert!(max_glyph.validate(usize::MAX).is_ok());
    assert!(mui_text::text_run(&font, "A", max_size, &[], 0.1).is_err());
    println!("PASS: f32::MAX glyph path remains valid while non-finite text metrics are rejected");

    assert!(mui_text::glyph_path(&font, 'A', f64::MAX, &[], 0.1).is_err());
    assert!(mui_text::text_run(&font, "A", f64::MAX, &[], 0.1).is_err());
    println!("PASS: f64::MAX is rejected before crossing the f32 size boundary");
}
