use super::xcb_connection::X11Connection;
use crate::platform::*;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{Colormap, ColormapAlloc, ConnectionExt, VisualClass, Visualid};

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
        connection: &X11Connection, gl_config: Option<crate::gl::GlConfig>, transparent: bool,
    ) -> Result<Self> {
        let Some(gl_config) = gl_config else {
            return Self::find_best_visual_config(connection, transparent);
        };
        let (fb_config, window_config) =
            super::gl::GlContextInner::get_fb_config_and_visual(connection, gl_config)?;
        Ok(Self {
            fb_config: Some(fb_config),
            visual_depth: window_config.depth,
            visual_id: window_config.visual,
            color_map: Some(create_color_map(connection, window_config.visual)?),
        })
    }

    pub fn find_best_visual_config(connection: &X11Connection, transparent: bool) -> Result<Self> {
        let screen = connection.default_screen();
        // Only an explicitly translucent detached window needs ARGB. Keep the
        // opaque root visual for editors and servers without a suitable visual.
        let alpha_visual = transparent
            .then(|| {
                screen.allowed_depths.iter().find(|depth| depth.depth == 32).and_then(|depth| {
                    depth.visuals.iter().find(|v| v.class == VisualClass::TRUE_COLOR)
                })
            })
            .flatten();
        let (visual_id, visual_depth) = alpha_visual
            .map_or((screen.root_visual, screen.root_depth), |visual| (visual.visual_id, 32));
        Ok(Self {
            #[cfg(feature = "opengl")]
            fb_config: None,
            visual_id,
            visual_depth,
            color_map: Some(create_color_map(connection, visual_id)?),
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
