//! Synthetic X11 parent/child surface test, not a DAW or GPUI runtime test.
use crate::{fixtures, gscene, Result};
use std::{path::Path, sync::Arc};
use winit::platform::x11::WindowAttributesExtX11;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use x11rb::protocol::xproto::ConnectionExt;
fn xid(w: &winit::window::Window) -> Result<u32> {
    match w.window_handle()?.as_raw() {
        RawWindowHandle::Xlib(h) => Ok(h.window as u32),
        RawWindowHandle::Xcb(h) => Ok(h.window.get()),
        _ => Err("X11 handle required".into()),
    }
}
#[allow(deprecated)]
pub fn run(out: &Path) -> Result<()> {
    let events = winit::event_loop::EventLoop::new()?;
    let parent = Arc::new(
        events.create_window(
            winit::window::Window::default_attributes()
                .with_title("Synthetic plugin host")
                .with_inner_size(winit::dpi::PhysicalSize::new(1000, 650)),
        )?,
    );
    let parent_id = xid(&parent)?;
    let (conn, _) = x11rb::connect(None)?;
    let cx: gpui_wgpu::GpuContext = Default::default();
    let marks = fixtures::fixture()?;
    let scene = gscene(&marks, 0.5, false)?;
    let mut windows = Vec::new();
    let mut renderers = Vec::new();
    for x in [0, 400] {
        let child = Arc::new(
            events.create_window(
                winit::window::Window::default_attributes()
                    .with_embed_parent_window(parent_id)
                    .with_inner_size(winit::dpi::PhysicalSize::new(384, 256))
                    .with_position(winit::dpi::PhysicalPosition::new(x, 0)),
            )?,
        );
        assert_eq!(conn.query_tree(xid(&child)?)?.reply()?.parent, parent_id);
        let renderer = gpui_wgpu::WgpuRenderer::new(
            cx.clone(),
            &child,
            gpui_wgpu::WgpuSurfaceConfig {
                size: gpui::size(gpui::DevicePixels(384), gpui::DevicePixels(256)),
                transparent: false,
                preferred_present_mode: Some(gpui_wgpu::wgpu::PresentMode::Immediate),
            },
            None,
        )?;
        windows.push(child);
        renderers.push(renderer);
    }
    for _ in 0..5 {
        for r in &mut renderers {
            assert!(r.draw(&scene));
        }
    }
    // Closing one editor must not destroy the context used by its sibling.
    renderers.pop();
    windows.pop();
    assert!(renderers[0].draw(&scene));
    for (w, h) in [(480, 320), (384, 256), (600, 400)] {
        let _ = windows[0].request_inner_size(winit::dpi::PhysicalSize::new(w, h));
        renderers[0].update_drawable_size(gpui::size(gpui::DevicePixels(w), gpui::DevicePixels(h)));
        assert!(renderers[0].draw(&scene));
    }
    let c = cx.borrow();
    let c = c.as_ref().unwrap();
    c.device
        .poll(gpui_wgpu::wgpu::PollType::wait_indefinitely())?;
    let report = serde_json::json!({"adapter":format!("{:?}",c.adapter.get_info()),"verified_parent_relationships":2,"shared_device":true,"sibling_survives_close":true,"resize_draws":3,"scope":"gpui_wgpu rendering in two winit X11 children of a synthetic parent; no GPUI App runtime, DAW, automation, focus or platform portability validation"});
    std::fs::write(
        out.join("embedding.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    Ok(())
}
