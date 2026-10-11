//! Run on a startup/worker thread, never in an audio callback:
//! cargo run -p mui-text --example system_fonts --features system-fonts
use mui_text::{Family, FontDatabase, FontQuery, shape_run};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let database = FontDatabase::discover_system();
    let families = database.families();
    let Some(family) = families.first() else {
        return Err("no readable installed fonts; supply bytes with FontDatabase::add".into());
    };
    let requested = [Family::Name(family)];
    let query = FontQuery {
        families: &requested,
        language: Some("ja"),
        ..FontQuery::default()
    };
    let text = "Hello 世界 😀";
    let selection = database.prepare(&query, text)?;
    let run = shape_run(&selection.fonts, text, 24., &selection.axes.to_vec())?;
    println!(
        "{} faces; primary {family}; {} selected; {} missing graphemes; width {}",
        database.len(),
        selection.fonts.len(),
        selection.missing.len(),
        run.advance,
    );
    // Keep this exact font order for rendering: each glyph's `font` is its index.
    // A Scene/Ui can use fonts[0] as primary and fonts[1..] as fallbacks.
    Ok(())
}
