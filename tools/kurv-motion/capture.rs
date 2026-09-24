// Included only in an isolated Kurv storybook build by export.py.
fn motion_export(scene: &mui2::scene::ResolvedScene, width: u16, height: u16, scale: f64) -> Result<(), String> {
    let Ok(directory) = std::env::var("MUI_MOTION_OUTPUT") else { return Ok(()); };
    let roots: Vec<String> = serde_json::from_str(&std::env::var("MUI_MOTION_PARTS").unwrap_or("[]".into())).map_err(|e| e.to_string())?;
    motion_export_to(scene, width, height, scale, std::path::Path::new(&directory), &roots)
}

fn motion_export_to(scene: &mui2::scene::ResolvedScene, width: u16, height: u16, scale: f64, directory: &std::path::Path, roots: &[String]) -> Result<(), String> {
    mui_motion_bridge::capture(scene, width, height, scale, directory, roots)
}

fn motion_resize(root: &mui2::prelude::El) -> Result<mui2::prelude::El, String> {
    let Ok(value) = std::env::var("MUI_MOTION_RESIZE") else { return Ok(root.clone()); };
    let (id, width, height): (String, f64, f64) = serde_json::from_str(&value).map_err(|e| e.to_string())?;
    mui2::scene::resize_capture(root, &id, Size::new(width,height)).map_err(|e|e.to_string())
}
