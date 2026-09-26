//! Platform-neutral input and time for a persistent native MUI editor.
use mui::prelude::*;
use serde_json::Value;

pub struct Editor {
    pub ui: Ui,
    pointer: PointerInput,
    sample_frame: u64,
    pub selection: Vec<String>,
    sizes: std::collections::BTreeMap<String, Size>,
}
impl Editor {
    pub fn layout(
        mut tree: El,
        sizes: &std::collections::BTreeMap<String, Size>,
    ) -> Result<El, String> {
        for (id, size) in sizes {
            tree = mui_scene::resize_capture(&tree, id, *size).map_err(|e| e.to_string())?;
        }
        Ok(tree)
    }
    pub fn new(ui: Ui) -> Self {
        Self {
            ui,
            pointer: PointerInput::default(),
            sample_frame: 0,
            selection: Vec::new(),
            sizes: std::collections::BTreeMap::default(),
        }
    }
    /// Run every input edge in order, then a neutral frame to consume edits.
    /// The caller retains its real view/model; only the host input is adapted.
    pub fn advance(
        &mut self,
        commands: &[Value],
        sample_frame: u64,
        mut frame: impl FnMut(
            &mut Ui,
            Input,
            f64,
            &std::collections::BTreeMap<String, Size>,
        ) -> Result<(), String>,
    ) -> Result<(), String> {
        let dt = (sample_frame.saturating_sub(self.sample_frame) as f64 / 48_000.).min(1.);
        self.sample_frame = sample_frame;
        for command in commands {
            if command["kind"] == "select" {
                let ids = command["ids"].as_array().ok_or("Expected surface ids")?;
                if ids.len() > 32 {
                    return Err("Select at most 32 independent surfaces".into());
                }
                let selection = ids
                    .iter()
                    .map(|v| {
                        v.as_str()
                            .filter(|s| !s.is_empty() && s.len() < 256)
                            .map(str::to_owned)
                            .ok_or("Invalid surface id".to_string())
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                if let Some(scene) = self.ui.scene() {
                    scene
                        .capture_layers(&selection.iter().map(String::as_str).collect::<Vec<_>>())
                        .map_err(|e| e.to_string())?;
                }
                self.selection = selection;
                continue;
            }
            if command["kind"] == "resize" {
                let id = command["id"].as_str().ok_or("Missing surface id")?;
                if command["reset"] == true {
                    self.sizes.remove(id);
                } else {
                    let size = Size::new(
                        number(command, "width", 8192.)?,
                        number(command, "height", 8192.)?,
                    );
                    if size.width < 8. || size.height < 8. {
                        return Err("Size must be at least 8".into());
                    }
                    if !mui_scene::Id::is_named(id)
                        || self.ui.scene().and_then(|s| s.surface(id)).is_none()
                    {
                        return Err("Resize requires a named native surface".into());
                    }
                    self.sizes.insert(id.to_owned(), size);
                }
                continue;
            }
            if command["kind"] == "cancel" {
                self.ui.cancel();
                self.pointer = PointerInput::default();
                frame(&mut self.ui, Input::default(), 0., &self.sizes)?;
                continue;
            }
            let input = decode(command, self.pointer)?;
            self.pointer = input.pointer;
            frame(&mut self.ui, input, 0., &self.sizes)?;
        }
        frame(&mut self.ui, Input::from(self.pointer), dt, &self.sizes)
    }
}
fn number(v: &Value, key: &str, limit: f64) -> Result<f64, String> {
    v[key]
        .as_f64()
        .filter(|x| x.is_finite() && x.abs() <= limit)
        .ok_or_else(|| format!("Invalid input {key}"))
}
fn decode(v: &Value, mut pointer: PointerInput) -> Result<Input, String> {
    pointer.mods = Mods {
        shift: v["shift"].as_bool().unwrap_or(false),
        ctrl: v["ctrl"].as_bool().unwrap_or(false),
        alt: v["alt"].as_bool().unwrap_or(false),
        cmd: v["meta"].as_bool().unwrap_or(false),
    };
    let mut input = Input::from(pointer);
    match v["kind"].as_str() {
        Some("pointer") => {
            input.pointer.pos = Some(Point::new(
                number(v, "x", 100_000.)?,
                number(v, "y", 100_000.)?,
            ));
            let buttons = v["buttons"]
                .as_u64()
                .filter(|b| *b <= 7)
                .ok_or("Invalid buttons")?;
            input.pointer.buttons = Buttons::default()
                .set(Button::Primary, buttons & 1 != 0)
                .set(Button::Secondary, buttons & 2 != 0)
                .set(Button::Middle, buttons & 4 != 0);
        }
        Some("wheel") => {
            input.wheel = Point::new(number(v, "dx", 10000.)?, number(v, "dy", 10000.)?);
        }
        Some("key") => {
            let key = v["key"].as_str().ok_or("Missing key")?;
            // The W3C key name, except that its space bar is the character " ".
            let mut chars = key.chars();
            let key = match (key, chars.next(), chars.next()) {
                (" ", ..) => Key::Space,
                (_, Some(c), None) => Key::Char(c),
                _ => Key::from_name(key).ok_or("Unsupported key")?,
            };
            input.keys.push(KeyPress {
                key,
                mods: input.pointer.mods,
            });
            if let Some(text) = v["text"].as_str() {
                if text.len() > 4096 {
                    return Err("Text too long".into());
                }
                input.text = text.into();
            }
        }
        _ => return Err("Unknown native input".into()),
    }
    Ok(input)
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn ordered_edges_time_and_validation() {
        let mut editor = Editor::new(Ui::new(Theme::DEFAULT));
        let mut frames = Vec::new();
        editor
            .advance(
                &[
                    json!({"kind":"pointer","x":10,"y":20,"buttons":1}),
                    json!({"kind":"pointer","x":30,"y":40,"buttons":0}),
                ],
                1600,
                |_, i, dt, _| {
                    frames.push((i.pointer.buttons.is_empty(), dt));
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(frames, vec![(false, 0.), (true, 0.), (true, 1. / 30.)]);
        assert!(
            decode(
                &json!({"kind":"pointer","x":1e30,"y":0,"buttons":1}),
                PointerInput::default()
            )
            .is_err()
        );
        editor
            .advance(&[json!({"kind":"cancel"})], 3200, |_, i, _, _| {
                assert!(i.pointer.buttons.is_empty());
                Ok(())
            })
            .unwrap();
    }
}
