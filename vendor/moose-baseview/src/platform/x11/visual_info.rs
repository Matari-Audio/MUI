use super::xcb_connection::X11Connection;
use crate::platform::*;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{Colormap, ColormapAlloc, ConnectionExt, Visualid};

pub(crate) struct WindowVisualConfig {
    #[cfg(feature = "opengl")]
    pub fb_config: Option<super::gl::FbConfig>,
    pub visual_depth: u8,
    pub visual_id: Visualid,
    pub color_map: Option<Colormap>,
}

impl WindowVisualConfig {
    #[cfg(feature = "opengl")]
    pub fn find_best_visual_config_for_gl(
        connection: &X11Connection, gl_config: Option<crate::gl::GlConfig>,
    ) -> Result<Self> {
        let Some(gl_config) = gl_config else { return Self::find_best_visual_config(connection) };
        let (fb_config, window_config) =
            super::gl::GlContextInner::get_fb_config_and_visual(connection, gl_config)?;
        Ok(Self {
            fb_config: Some(fb_config),
            visual_depth: window_config.depth,
            visual_id: window_config.visual,
            color_map: Some(create_color_map(connection, window_config.visual)?),
        })
    }

    pub fn find_best_visual_config(connection: &X11Connection) -> Result<Self> {
        let screen = connection.default_screen();
        // Editors are opaque. Prefer the server's root visual, not arbitrary
        // ARGB32: without compositing a zero alpha channel can hide the editor.
        // An explicit visual/colormap also works under a different-depth host
        // parent, and is reported accurately in Vulkan's raw window handle.
        Ok(Self {
            #[cfg(feature = "opengl")]
            fb_config: None,
            visual_id: screen.root_visual,
            visual_depth: screen.root_depth,
            color_map: Some(create_color_map(connection, screen.root_visual)?),
        })
    }
}

fn create_color_map(connection: &X11Connection, visual_id: Visualid) -> Result<Colormap> {
    let colormap = connection.conn.generate_id()?;
    connection
        .conn
        .create_colormap(
            ColormapAlloc::NONE,
            colormap,
            connection.default_screen().root,
            visual_id,
        )?
        .check()?;
    Ok(colormap)
}
