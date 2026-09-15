use mui_layout::generic::{column, leaf, resolve, Align, Limits, Size};
fn main() -> Result<(), mui_layout::Error> {
    let d = 28.;
    let gap = 10.;
    let inset = 12.;
    let controls = column([
        leaf(d, d).id("plus"),
        leaf(d, d).id("pie-a"),
        leaf(d, d).id("pie-b"),
    ])
    .id("controls")
    .gap(gap);
    let pill = column([controls]).id("pill").pad(10.);
    let tab = column([pill])
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
