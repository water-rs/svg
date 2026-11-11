//! Renders the same SVG through the GPU engine and the CPU rasteriser.
//!
//! One `Drawing` feeds `OffscreenRenderer::new()` (the engine a real surface
//! uses) and `OffscreenRenderer::cpu()` (what pictures go through on hosts that
//! rasterise for a platform image view). Comparing them side by side is the
//! parity check between the two backends.
//!
//! The PNGs are written out to be looked at. The backends rasterise
//! differently, so nothing about them is asserted pixel-wise.

use std::path::PathBuf;

#[cfg(feature = "text")]
use waterui_core::layout::Size;
use waterui_core::layout::{Affine2, Rect};
use waterui_graphics::canvas::Drawing;
use waterui_graphics::color::WorkingColor;
use waterui_graphics::{OffscreenRenderer, OffscreenSize};
use waterui_svg::Svg;
#[cfg(feature = "text")]
use waterui_svg::usvg::fontdb;

const STROKED_ICON: &str = include_str!("data/stroked_icon.svg");
const PAINTED_ICON: &str = include_str!("data/painted_icon.svg");
const PAINT_TRANSFORM_ICON: &str = include_str!("data/paint_transform_icon.svg");
const TINTED_ICON: &str = include_str!("data/tinted_icon.svg");
// Two documents whose artwork is `<text>`. Nothing supplies the parser with a
// font here, so what they draw is their background rectangle and no glyphs —
// the same picture whichever `usvg` font features are compiled in.
const GENERIC_FAMILY_TEXT: &str = include_str!("data/text_generic_family.svg");
const NAMED_FAMILY_TEXT: &str = include_str!("data/text_named_family.svg");
// The one font those fixtures are laid out with, bundled so that the drawing is
// the same on every machine rather than whatever the host has installed.
#[cfg(feature = "text")]
const ROBOTO: &[u8] = include_bytes!("fonts/Roboto-Regular.ttf");

const ORANGE: WorkingColor = WorkingColor::new([0.9, 0.25, 0.0, 1.0]);
const TEAL: WorkingColor = WorkingColor::new([0.0, 0.45, 0.5, 1.0]);

/// The directory rendered PNGs land in, under `target/` so they are never
/// committed.
fn artifacts_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/svg-artifacts/render");
    std::fs::create_dir_all(&dir).expect("output directory must be creatable");
    dir
}

/// `drawing` on white, in a box of its own size: the images are reviewed by
/// eye, and a transparent ground lets a hole read as the absence it is.
fn on_white(drawing: &Drawing) -> Drawing {
    let size = drawing.size();
    Drawing::record(size, |cx| {
        cx.fill(Rect::from_size(size), WorkingColor::WHITE);
        cx.draw(drawing, Affine2::IDENTITY);
    })
}

/// Renders `drawing` on both engines, `scale` pixels to the point.
fn render_both(drawing: &Drawing, scale: f32, name: &str) {
    let size = drawing.size();
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "test sizes are small positive numbers"
    )]
    let pixels = OffscreenSize::try_from_pixels(
        (size.width * scale).round() as u32,
        (size.height * scale).round() as u32,
    )
    .expect("test size must be valid");
    let gpu = OffscreenRenderer::new().expect("offscreen render requires a GPU engine");
    let cpu = OffscreenRenderer::cpu().expect("offscreen render requires the CPU rasteriser");
    let drawing = on_white(drawing);

    for (engine, image) in [
        (
            "gpu",
            gpu.render_drawing(&drawing, pixels)
                .expect("GPU render should succeed"),
        ),
        (
            "cpu",
            cpu.render_drawing(&drawing, pixels)
                .expect("CPU render should succeed"),
        ),
    ] {
        image
            .save_png(artifacts_dir().join(format!("{name}_{engine}.png")))
            .expect("png should be written");
    }
}

#[test]
fn gpu_and_cpu_render_an_svg() {
    for (content, name, scale) in [
        (STROKED_ICON, "stroked", 8.0),
        (PAINTED_ICON, "painted", 4.0),
        (PAINT_TRANSFORM_ICON, "paint_transform", 6.0),
        (GENERIC_FAMILY_TEXT, "text_generic_family", 3.0),
        (NAMED_FAMILY_TEXT, "text_named_family", 3.0),
    ] {
        render_both(&Svg::new(content).drawing(WorkingColor::BLACK), scale, name);
    }
}

/// The same document in two tints: everything that asks for the tint — the
/// star, the gradient's first stop, the translucent block, the dashed ring —
/// follows it, and everything the document colours itself stays.
#[test]
fn the_tint_paints_what_asks_for_it() {
    let svg = Svg::new(TINTED_ICON).viewbox(96.0, 32.0);
    render_both(&svg.drawing(ORANGE), 6.0, "tinted_orange");
    render_both(&svg.drawing(TEAL), 6.0, "tinted_teal");
}

/// Path data is drawn in the tint, filled or stroked.
#[test]
fn path_data_is_drawn_in_the_tint() {
    render_both(
        &Svg::from_path(
            "M12 2L15.09 8.26L22 9.27L17 14.14L18.18 21.02L12 17.77L5.82 21.02L7 14.14L2 9.27L8.91 8.26Z",
            24.0,
            24.0,
        )
        .drawing(ORANGE),
        8.0,
        "path_filled",
    );
    render_both(
        &Svg::from_stroke_path("M3 12h18M3 6h18M3 18h18", 24.0, 24.0).drawing(TEAL),
        8.0,
        "path_stroked",
    );
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
    let mut fonts = fontdb::Database::new();
    fonts.load_font_data(ROBOTO.to_vec());
    fonts.set_sans_serif_family("Roboto");
    let fonts = std::sync::Arc::new(fonts);

    for (content, name) in [
        (GENERIC_FAMILY_TEXT, "text_generic_family"),
        (NAMED_FAMILY_TEXT, "text_named_family"),
    ] {
        let drawing = Svg::new(content)
            .fonts(fonts.clone())
            .drawing(WorkingColor::BLACK);
        assert_eq!(drawing.size(), Size::new(96.0, 32.0));
        render_both(&drawing, 3.0, &format!("{name}_with_fonts"));
    }
}
