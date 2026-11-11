//! `mui-preview --list-scenes | --scene NAME --snap out.png | --tree ...`:
//! every gallery scene through `mui-headless`, no window, no GPU.
use mui::prelude::*;
use mui_headless::Scene;

use crate::{scenes, skin};

/// One headless scene per gallery scene: the specimen centred on the stage
/// background, in the gallery's skin and fonts, at the gallery's default size.
pub fn all() -> Vec<Scene> {
    scenes::all()
        .into_iter()
        .map(|mut scene| {
            let font = Font::new(epaint_default_fonts::HACK_REGULAR).expect("bundled Hack parses");
            let ui = Ui::new(skin::SKIN).font(font);
            Scene::new(scene.name(), (760, 560), ui, move |ui| {
                stack([scene.specimen(ui).centered()]).fill(Role::Background)
            })
        })
        .collect()
}
