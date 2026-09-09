use waterui_core::layout::Size;
use waterui_graphics::{Scene2D, SceneContent};

use crate::scene_data::SvgSceneData;

/// Scene content that draws one SVG document.
#[derive(Debug)]
pub struct SvgSceneContent {
    scene_data: SvgSceneData,
}

impl SvgSceneContent {
    /// Parses `svg_content` into content that draws it.
    ///
    /// The parser is given no fonts, so a `<text>` element is laid out against
    /// an empty font database and contributes nothing to the drawing.
    #[must_use]
    pub fn new(svg_content: &str) -> Self {
        Self {
            scene_data: SvgSceneData::parse(svg_content),
        }
    }

    /// Parses `svg_content` into content that draws it, laying `<text>` out
    /// with `fonts`.
    ///
    /// The text becomes outline paths like the rest of the drawing, so it
    /// renders identically on every backend rather than through a platform
    /// text stack. Which fonts exist is the caller's business: this crate never
    /// reads the operating system's font catalogue, so an application that
    /// wants system fonts loads them into the database itself and passes the
    /// result here.
    #[cfg(feature = "text")]
    #[must_use]
    pub fn with_fonts(
        svg_content: &str,
        fonts: alloc::sync::Arc<crate::usvg::fontdb::Database>,
    ) -> Self {
        Self {
            scene_data: SvgSceneData::parse_with_fonts(svg_content, fonts),
        }
    }
}

impl SceneContent for SvgSceneContent {
    fn build_scene(&mut self, scene: &mut dyn Scene2D, width: f32, height: f32) -> bool {
        self.scene_data.draw(scene, width, height);
        false
    }

    fn intrinsic_size(&self) -> Option<Size> {
        Some(self.scene_data.intrinsic_size())
    }
}
