//! Renders the same SVG through the GPU engine and the CPU rasteriser.
//!
//! One drawing feeds `OffscreenRenderer<Gpu>` (the engine a real surface uses)
//! and `OffscreenRenderer<Raster>` (what a `Picture` goes through on hosts
//! that rasterise for a platform image view). Comparing them side by side is
//! the port's parity check between the two backends.
//!
//! The PNGs are written out to be looked at. The backends rasterise
//! differently, so nothing about them is asserted pixel-wise.

use std::path::PathBuf;

use waterui_graphics::{OffscreenRenderer, OffscreenSize, SceneContent};
use waterui_svg::SvgSceneContent;
#[cfg(feature = "text")]
use waterui_svg::usvg::fontdb;

const STROKED_ICON: &str = include_str!("data/stroked_icon.svg");
const PAINTED_ICON: &str = include_str!("data/painted_icon.svg");
// Two documents whose artwork is `<text>`. Nothing supplies the parser with a
// font here, so what they draw is their background rectangle and no glyphs —
// the same picture whichever `usvg` font features are compiled in.
const GENERIC_FAMILY_TEXT: &str = include_str!("data/text_generic_family.svg");
const NAMED_FAMILY_TEXT: &str = include_str!("data/text_named_family.svg");
// The one font those fixtures are laid out with, bundled so that the drawing is
// the same on every machine rather than whatever the host has installed.
#[cfg(feature = "text")]
const ROBOTO: &[u8] = include_bytes!("fonts/Roboto-Regular.ttf");

/// The directory rendered PNGs land in, under `target/` so they are never
/// committed.
fn artifacts_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/svg-artifacts/scene_engines")
}

fn render_and_save(
    renderer: &impl Fn(&mut dyn SceneContent) -> waterui_graphics::OffscreenImage,
    mut content: SvgSceneContent,
    name: &str,
) {
    let image = renderer(&mut content);
    image
        .save_png(artifacts_dir().join(format!("{name}.png")))
        .expect("png should be written");
}

#[test]
fn gpu_and_cpu_render_an_svg() {
    let dir = artifacts_dir();
    std::fs::create_dir_all(&dir).expect("output directory must be creatable");
    let size = OffscreenSize::try_from_pixels(192, 192).expect("test size must be valid");
    let gpu = OffscreenRenderer::new().expect("offscreen render requires a GPU engine");
    let cpu = OffscreenRenderer::cpu().expect("offscreen render requires the CPU rasteriser");

    for (content, icon_name) in [
        (STROKED_ICON, "stroked"),
        (PAINTED_ICON, "painted"),
        (GENERIC_FAMILY_TEXT, "text_generic_family"),
        (NAMED_FAMILY_TEXT, "text_named_family"),
    ] {
        render_and_save(
            &|content| {
                gpu.render(content, size, 1.0)
                    .expect("GPU render should succeed")
            },
            SvgSceneContent::new(content),
            &format!("{icon_name}_gpu"),
        );
        render_and_save(
            &|content| {
                cpu.render(content, size, 1.0)
                    .expect("CPU render should succeed")
            },
            SvgSceneContent::new(content),
            &format!("{icon_name}_cpu"),
        );
    }
}

/// The same text fixtures, drawn with a font database behind them.
///
/// This is what the `text` feature buys: `usvg` lays the `<text>` element out
/// and flattens it into outlines, and those outlines draw like any other path.
/// The font is one file bundled beside these tests and handed over as bytes, so
/// the picture is the same on every machine and the library needs none of
/// `usvg`'s filesystem font features to draw it.
#[cfg(feature = "text")]
#[test]
fn text_draws_when_the_caller_supplies_fonts() {
    let dir = artifacts_dir();
    std::fs::create_dir_all(&dir).expect("output directory must be creatable");
    let size = OffscreenSize::try_from_pixels(192, 192).expect("test size must be valid");
    let gpu = OffscreenRenderer::new().expect("offscreen render requires a GPU engine");

    let mut fonts = fontdb::Database::new();
    fonts.load_font_data(ROBOTO.to_vec());
    fonts.set_sans_serif_family("Roboto");
    let fonts = std::sync::Arc::new(fonts);

    for (content, icon_name) in [
        (GENERIC_FAMILY_TEXT, "text_generic_family"),
        (NAMED_FAMILY_TEXT, "text_named_family"),
    ] {
        render_and_save(
            &|content| {
                gpu.render(content, size, 1.0)
                    .expect("GPU render should succeed")
            },
            SvgSceneContent::with_fonts(content, fonts.clone()),
            &format!("{icon_name}_with_fonts"),
        );
    }
}
