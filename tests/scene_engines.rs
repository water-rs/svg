//! Renders the fixture SVGs through `Engine<Gpu>` into an offscreen target.
//!
//! An icon records Cherenkov content, and this is that content drawn by the
//! engine WaterUI ships: the PNGs are written out to be looked at, and nothing
//! about the rasterization is asserted pixel-wise.

use waterui_graphics::offscreen::{OffscreenRenderer, OffscreenSize};
use waterui_svg::SvgSceneContent;
#[cfg(feature = "text")]
use waterui_svg::usvg::fontdb;
use waterui_testing::TestArtifacts;

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

const SIDE: u32 = 192;

fn export(
    artifacts: &TestArtifacts,
    renderer: &OffscreenRenderer,
    mut content: SvgSceneContent,
    name: &str,
) {
    let size = OffscreenSize::try_from_pixels(SIDE, SIDE).expect("test size must be valid");
    let output = renderer
        .render(&mut content, size, 1.0)
        .expect("offscreen render should succeed");
    output
        .save_png(artifacts.snapshot_path("scene_engines", name))
        .expect("png should be written");
}

#[test]
fn the_engine_renders_an_svg() {
    let artifacts = TestArtifacts::new("svg");
    std::fs::create_dir_all(artifacts.case_dir("scene_engines"))
        .expect("output directory must be creatable");
    let renderer = OffscreenRenderer::new().expect("svg export requires a GPU adapter");

    for (content, icon_name) in [
        (STROKED_ICON, "stroked"),
        (PAINTED_ICON, "painted"),
        (GENERIC_FAMILY_TEXT, "text_generic_family"),
        (NAMED_FAMILY_TEXT, "text_named_family"),
    ] {
        export(
            &artifacts,
            &renderer,
            SvgSceneContent::new(content),
            icon_name,
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
    let artifacts = TestArtifacts::new("svg");
    std::fs::create_dir_all(artifacts.case_dir("scene_engines"))
        .expect("output directory must be creatable");
    let renderer = OffscreenRenderer::new().expect("svg export requires a GPU adapter");

    let mut fonts = fontdb::Database::new();
    fonts.load_font_data(ROBOTO.to_vec());
    fonts.set_sans_serif_family("Roboto");
    let fonts = std::sync::Arc::new(fonts);

    for (content, icon_name) in [
        (GENERIC_FAMILY_TEXT, "text_generic_family"),
        (NAMED_FAMILY_TEXT, "text_named_family"),
    ] {
        export(
            &artifacts,
            &renderer,
            SvgSceneContent::with_fonts(content, fonts.clone()),
            &format!("{icon_name}_with_fonts"),
        );
    }
}
