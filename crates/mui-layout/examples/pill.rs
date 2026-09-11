use mui_layout::{resolve, Align, Limits, Node, Size};
fn main() -> Result<(), mui_layout::Error> {
    let d = 28.;
    let gap = 10.;
    let inset = 12.;
    let controls = Node::column(
        "controls",
        [
            Node::leaf("plus", Size::new(d, d)),
            Node::leaf("pie-a", Size::new(d, d)),
            Node::leaf("pie-b", Size::new(d, d)),
        ],
    )
    .gap(gap);
    let pill = Node::column("pill", [controls]).padding(10.);
    let tab = Node::column("tab", [pill])
        .padding(inset)
        .min_size(Size::new(92., 0.))
        .align(Align::Stretch);
    let result = resolve(&tab, None, Limits::default())?;
    for (key, frame) in result.frames() {
        println!("{key}: {frame:?}");
    }
    Ok(())
}
