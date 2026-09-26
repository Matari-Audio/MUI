use mui_layout::{Align, Limits, Size, col, block, resolve};
fn main() -> Result<(), mui_layout::Error> {
    let d = 28.;
    let gap = 10.;
    let inset = 12.;
    let controls = col([
        block(d, d).id("plus"),
        block(d, d).id("pie-a"),
        block(d, d).id("pie-b"),
    ])
    .id("controls")
    .gap(gap);
    let pill = col([controls]).id("pill").pad(10.);
    let tab = col([pill])
        .id("tab")
        .pad(inset)
        .min_size(Size::new(92., 0.))
        .align(Align::Stretch);
    let result = resolve(&tab, None, Limits::default())?;
    for (key, frame) in result.frames() {
        println!("{key}: {frame:?}");
    }
    Ok(())
}
