#[cfg(test)]
mod compiler_contract;
mod generated;
#[cfg(test)]
mod generated_items;

fn main() {
    let scene = mui_core::resolve_scene(&generated::generated_scene())
        .expect("resolve generated MUI scene");
    let tab = scene.surface("tab").expect("tab");
    let pill = scene.surface("pill-shell").expect("pill-shell");
    let outer = scene.surface("outer").expect("outer");
    let t = tab.analytic_rect.expect("analytic tab");
    let p = pill.analytic_rect.expect("analytic inset pill shell");
    println!(
        "layout={:.1}x{:.1}",
        scene.layout.size.width, scene.layout.size.height
    );
    println!(
        "tab={:.1}x{:.1} r={:.1}",
        t.bounds().width(),
        t.bounds().height(),
        t.radius()
    );
    println!(
        "inset={:.1}x{:.1} r={:.1}",
        p.bounds().width(),
        p.bounds().height(),
        p.radius()
    );
    println!("parallel_thickness={:.1}", t.radius() - p.radius());
    println!(
        "outer_components={} outer_vertices={}",
        outer.basis.components(),
        outer.basis.vertex_count()
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generated_typescript_scene_resolves() {
        let ui = generated_items::generated_ui().unwrap();
        let items = ui
            .resolve_with(|_, text, _| Ok(mui_layout::Size::new(text.len() as f64 * 8., 20.)))
            .unwrap();
        assert_eq!(
            items.surface("filter").unwrap().bounds.unwrap().max.y,
            items.layout.frame("panel").unwrap().y
        );
        assert_eq!(
            ui.info("filter").unwrap().tap.as_deref(),
            Some("select-filter")
        );
        let contract = mui_core::resolve_scene(&compiler_contract::generated_scene()).unwrap();
        assert!(contract
            .layout
            .frame("tab\u{1}\u{8}\u{c}\\\"שלום🎹")
            .is_some());
        assert_eq!(contract.surface("shell").unwrap().basis.components(), 1);
        assert!(compiler_contract::generated_scene().theme.colors().is_ok());
        let scene = mui_core::resolve_scene(&generated::generated_scene()).unwrap();
        let tab = scene.surface("tab").unwrap().analytic_rect.unwrap();
        let pill = scene.surface("pill-shell").unwrap().analytic_rect.unwrap();
        assert!((tab.radius() - pill.radius() - 12.0).abs() < 1e-9);
        assert_eq!(scene.surface("outer").unwrap().basis.components(), 1);
        assert!(scene.surface("outer").unwrap().basis.vertex_count() >= 6);
    }
}
