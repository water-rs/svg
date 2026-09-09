//! Renders the same SVG through both scene engines.
//!
//! Icons are the reason the hybrid engine exists: an icon draws through
//! `Scene2D`, and on a device without indirect execution the compute pipeline
//! takes the process down rather than drawing it. This machine's GPU can run
//! either engine, which is how the two are compared here.
//!
//! The PNGs are written out to be looked at. The engines rasterize
//! differently, so nothing about them is asserted pixel-wise.

use kurbo::Affine;
use waterui_graphics::scene2d_cpu::rasterize_recording;
use waterui_graphics::shared_context::SceneEngine;
use waterui_graphics::{
    GpuRuntime, OffscreenRenderConfig, OffscreenSize, SceneContent, SceneRecording, SceneView,
};
use waterui_svg::SvgSceneContent;
#[cfg(feature = "text")]
use waterui_svg::usvg::fontdb;
use waterui_testing::{Snapshot, TestArtifacts};

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

#[test]
fn both_scene_engines_render_an_svg() {
    let artifacts = TestArtifacts::new("svg");
    std::fs::create_dir_all(artifacts.case_dir("scene_engines"))
        .expect("output directory must be creatable");
    let runtime = pollster::block_on(GpuRuntime::new())
        .expect("scene engine comparison requires a working GPU runtime");
    let size = OffscreenSize::try_from_pixels(192, 192).expect("test size must be valid");

    for (content, icon_name) in [
        (STROKED_ICON, "stroked"),
        (PAINTED_ICON, "painted"),
        (GENERIC_FAMILY_TEXT, "text_generic_family"),
        (NAMED_FAMILY_TEXT, "text_named_family"),
    ] {
        for (engine, engine_name) in [
            (SceneEngine::Classic, "classic"),
            (SceneEngine::Hybrid, "hybrid"),
        ] {
            let surface = SceneView::new(SvgSceneContent::new(content)).into_gpu_surface();
            let config = OffscreenRenderConfig::new(size)
                .format(wgpu::TextureFormat::Rgba8Unorm)
                .scene_engine(engine);
            let mut env = waterui_core::Environment::new();
            let output = pollster::block_on(surface.render_offscreen(&runtime, config, &mut env))
                .expect("offscreen render should succeed");
            output
                .save_png(
                    artifacts.snapshot_path("scene_engines", format!("{icon_name}_{engine_name}")),
                )
                .expect("png should be written");
        }

        // The CPU rasteriser is what a `Picture` goes through on the FFI
        // backends, so it sits beside the two GPU engines for review. Its
        // pixels are premultiplied, which only softens the anti-aliased edges
        // of the exported PNG.
        let mut recording = SceneRecording::new();
        SvgSceneContent::new(content).build_scene(&mut recording, 192.0, 192.0);
        let bitmap = rasterize_recording(&recording, 192, 192, Affine::IDENTITY);
        Snapshot {
            width: bitmap.width(),
            height: bitmap.height(),
            rgba8: bitmap.into_data(),
        }
        .save_png(artifacts.snapshot_path("scene_engines", format!("{icon_name}_cpu")))
        .expect("png should be written");
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
    let runtime = pollster::block_on(GpuRuntime::new())
        .expect("scene engine comparison requires a working GPU runtime");
    let size = OffscreenSize::try_from_pixels(192, 192).expect("test size must be valid");

    let mut fonts = fontdb::Database::new();
    fonts.load_font_data(ROBOTO.to_vec());
    fonts.set_sans_serif_family("Roboto");
    let fonts = std::sync::Arc::new(fonts);

    for (content, icon_name) in [
        (GENERIC_FAMILY_TEXT, "text_generic_family"),
        (NAMED_FAMILY_TEXT, "text_named_family"),
    ] {
        let surface =
            SceneView::new(SvgSceneContent::with_fonts(content, fonts.clone())).into_gpu_surface();
        let config = OffscreenRenderConfig::new(size)
            .format(wgpu::TextureFormat::Rgba8Unorm)
            .scene_engine(SceneEngine::Classic);
        let mut env = waterui_core::Environment::new();
        let output = pollster::block_on(surface.render_offscreen(&runtime, config, &mut env))
            .expect("offscreen render should succeed");
        output
            .save_png(artifacts.snapshot_path("scene_engines", format!("{icon_name}_with_fonts")))
            .expect("png should be written");
    }
}
