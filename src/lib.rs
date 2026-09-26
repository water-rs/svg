//! SVG rendering for `WaterUI`.
//!
//! This crate provides `Svg`, a view for rendering SVG content using GPU-accelerated
//! rendering via `SceneView`.
//!
//! A document is parsed with `usvg` and drawn through the engine-independent
//! `Scene2D` contract, so it renders on whichever engine the backend supplies.
//!
//! # Text, fonts and cargo features
//!
//! Everything this crate draws is geometry: paths, images and — when a document
//! has text and a font to set it in — the outlines `usvg` flattens that text
//! into. It never asks the operating system for a font, so a drawing looks the
//! same on every backend and on a device that has no font catalogue at all.
//!
//! | Feature | Default | Effect |
//! | --- | --- | --- |
//! | `text` | off | Enables `usvg/text`, which lays `<text>` out into outline paths, and the font-taking [`SvgSceneContent::with_fonts`] constructor. Costs `fontdb`, `rustybuzz` and the `unicode-bidi` / `unicode-script` / `unicode-vo` crates. |
//!
//! `usvg` is depended on with `default-features = false`. Its `system-fonts`
//! and `memmap-fonts` defaults are deliberately not enabled: both only widen
//! `fontdb` towards the filesystem (directory and fontconfig scanning,
//! memory-mapped faces), and this crate never asks `fontdb` to read a file. An
//! application that wants the operating system's fonts owns that decision
//! itself — it builds the font database and passes it to
//! [`SvgSceneContent::with_fonts`].
//!
//! Without `text`, and with `text` but no fonts, a `<text>` element is dropped
//! during parsing and draws nothing. [`Svg`] itself always parses without
//! fonts: it is the icon primitive, and icon artwork is paths.

extern crate alloc;

mod scene_data;
mod scene_renderer;
mod title;
mod tree_renderer;

/// The SVG parser this crate draws from.
pub use usvg;

/// Scene content that draws an SVG document, for composing an SVG into a scene
/// that is not a whole view of its own.
pub use scene_renderer::SvgSceneContent;

use alloc::string::String;

use suiteki::Str;
use waterui_core::layout::Size;
use waterui_core::reactive::signal::IntoComputed;
use waterui_core::{AnyView, Computed, Environment, Signal, SignalExt, View, constant};
use waterui_graphics::Picture;
use waterui_graphics::color::{Color, WorkingColor, working};
use waterui_layout::frame::Frame;

/// A view for rendering SVG content using GPU-accelerated rendering.
///
/// The SVG data can be either:
/// - Full SVG markup
/// - Path data only (d attribute from SVG path element)
///
/// # Example
///
/// ```ignore
/// // From SVG path data (most common for icons)
/// Svg::from_path("M10 20v-6h4v6h5v-8h3L12 3 2 12h3v8z", 24.0, 24.0)
///
/// // Stroke-based icons (like Lucide)
/// Svg::from_stroke_path("M3 12h18M3 6h18M3 18h18", 24.0, 24.0)
/// ```
#[derive(Debug, Clone)]
#[must_use = "an `Svg` does nothing unless it is rendered as part of a view"]
pub struct Svg {
    /// SVG content (path data or full SVG markup).
    pub(crate) content: Str,
    /// Intrinsic width for aspect ratio.
    pub(crate) width: Option<f32>,
    /// Intrinsic height for aspect ratio.
    pub(crate) height: Option<f32>,
    /// Optional tint color (for monochrome icons).
    pub(crate) tint: Option<Color>,
    /// Whether to render as stroke (outline) rather than fill.
    pub(crate) stroke: bool,
}

impl Svg {
    /// Creates an SVG from raw SVG markup or path data.
    ///
    /// For icons, prefer `from_path` which provides explicit dimensions.
    pub fn new(content: impl Into<Str>) -> Self {
        Self {
            content: content.into(),
            width: None,
            height: None,
            tint: None,
            stroke: false,
        }
    }

    /// Creates an SVG from path data with explicit dimensions.
    ///
    /// This is the recommended constructor for filled icon SVGs where the
    /// path data comes from the `d` attribute of an SVG path element.
    pub fn from_path(path_data: impl Into<Str>, width: f32, height: f32) -> Self {
        Self {
            content: path_data.into(),
            width: Some(width),
            height: Some(height),
            tint: None,
            stroke: false,
        }
    }

    /// Creates an SVG from path data rendered as strokes (outlines).
    ///
    /// This is for stroke-based icon sets like Lucide where the path
    /// represents the outline of the icon, not a filled shape.
    ///
    /// The stroke uses:
    /// - `stroke-width: 2`
    /// - `stroke-linecap: round`
    /// - `stroke-linejoin: round`
    /// - `fill: none`
    pub fn from_stroke_path(path_data: impl Into<Str>, width: f32, height: f32) -> Self {
        Self {
            content: path_data.into(),
            width: Some(width),
            height: Some(height),
            tint: None,
            stroke: true,
        }
    }

    /// Sets the tint color for the SVG.
    pub fn tint(mut self, color: impl Into<Color>) -> Self {
        self.tint = Some(color.into());
        self
    }

    /// Sets the SVG's own coordinate system — the box its path data is drawn
    /// in, and the size it asks for when nothing else has an opinion.
    ///
    /// This is *not* a layout size. `.size(…)` from `ViewExt` puts the drawing
    /// in a box of your choosing and scales it to fit; this declares what the
    /// drawing's coordinates mean, so shrinking it here reframes the artwork
    /// rather than resizing it. A method named `size` here would shadow
    /// `ViewExt::size` — an inherent method wins over a trait one — which
    /// silently turned `icon.size(8.0, 8.0)` into a 24pt drawing crammed into
    /// an 8pt viewBox, spilling over whatever sat next to it.
    pub const fn viewbox(mut self, width: f32, height: f32) -> Self {
        self.width = Some(width);
        self.height = Some(height);
        self
    }

    /// Build full SVG document from path data if needed.
    ///
    /// The `color` parameter specifies the fill or stroke color for the SVG.
    fn build_svg_content(&self, color: &str) -> alloc::string::String {
        if let (Some(width), Some(height)) = (self.width, self.height) {
            let content = self.content.as_str();
            if content.trim_start().starts_with('<') {
                // Full SVG markup: support tint only for currentColor-driven assets.
                if content.contains("currentColor") {
                    content.replace("currentColor", color)
                } else {
                    self.content.to_string()
                }
            } else if self.stroke {
                // Stroke-based path (for outline icons like Lucide)
                alloc::format!(
                    r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" fill="none" stroke="{color}" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="{content}"/></svg>"#
                )
            } else {
                // Filled path data - wrap in SVG document
                alloc::format!(
                    r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" fill="{color}"><path d="{content}"/></svg>"#
                )
            }
        } else {
            // Assume full SVG content
            self.content.to_string()
        }
    }

    /// Records this SVG, drawn in `color`, at its intrinsic size.
    fn record(&self, color: &str) -> (Size, cherenkov::Picture) {
        let scene_data = scene_data::SvgSceneData::parse(&self.build_svg_content(color));
        let size = scene_data.intrinsic_size();
        let recording = Picture::record(|scene| scene_data.draw(scene, size.width, size.height));
        (size, recording)
    }

    /// The name the document gives itself: the root `<title>` of full markup.
    ///
    /// Path data carries no title, so an icon built from a `d` attribute stays
    /// unnamed until the application names it with `.a11y_label(…)` — which
    /// also wins over a title wherever both exist.
    fn accessible_name(&self) -> Option<String> {
        let content = self.content.as_str();
        content
            .trim_start()
            .starts_with('<')
            .then(|| title::document_title(content))
            .flatten()
    }

    /// A picture of this SVG offering the document's own name to a screen
    /// reader.
    fn picture(&self, size: Size, recording: impl IntoComputed<cherenkov::Picture>) -> Picture {
        let picture = Picture::new(size, recording);
        match self.accessible_name() {
            Some(name) => picture.labeled(name),
            None => picture,
        }
    }

    /// A picture of this SVG drawn in a fixed `color`.
    fn to_picture(&self, color: &str) -> Picture {
        let (size, recording) = self.record(color);
        self.picture(size, constant(recording))
    }

    /// A picture of this SVG whose drawing follows `color_signal`: a new colour
    /// records new commands into the same picture.
    fn to_reactive_picture<S>(&self, color_signal: &S) -> Picture
    where
        S: Signal<Output = String> + 'static,
        S::Guard: 'static,
    {
        let (size, _) = self.record("#000000");
        let svg = self.clone();
        self.picture(size, color_signal.map(move |color| svg.record(&color).1))
    }

    /// Wraps a view in a frame carrying the SVG's intrinsic size.
    ///
    /// The intrinsic size is what the drawing asks for when nothing else has an
    /// opinion, so it is the frame's *ideal* extent, not its pinned one.
    /// Pinning it made `.size(…)` on an icon impossible to honour: the frame
    /// reported the natural size whatever it was offered, and the icon drew
    /// outside the box it had been given.
    ///
    /// It is also the frame's *maximum*, because the natural size is a ceiling
    /// as well as a default: a 24pt icon in a 44pt row is a 24pt icon. The
    /// scene the frame wraps takes whatever extent it is proposed — that is
    /// right for a `Canvas`, and it is why an icon needs the ceiling said out
    /// loud. Without it a stack that proposes its row height (`HStack` proposes
    /// the bounds height to a child that does not stretch) would get an icon
    /// stretched to that height. There is deliberately no *minimum*: the
    /// drawing stays free to shrink into a smaller box.
    fn frame_view(&self, view: impl View) -> AnyView {
        match (self.width, self.height) {
            (Some(w), Some(h)) => AnyView::new(
                Frame::new(view)
                    .ideal_width(w)
                    .ideal_height(h)
                    .max_width(w)
                    .max_height(h),
            ),
            _ => AnyView::new(Frame::new(view)),
        }
    }

    /// Creates a framed SVG scene view for the given color.
    fn to_framed_picture(&self, color: &str) -> AnyView {
        self.frame_view(self.to_picture(color))
    }

    /// Creates a framed reactive SVG scene view.
    fn to_reactive_framed_picture<S>(&self, color_signal: &S) -> AnyView
    where
        S: Signal<Output = String> + 'static,
        S::Guard: 'static,
    {
        self.frame_view(self.to_reactive_picture(color_signal))
    }

    /// Format a working-space colour as an SVG-compatible color string.
    ///
    /// Converts from the linear working space to gamma-encoded sRGB.
    /// - Opaque colors are emitted as `#rrggbb`.
    /// - Translucent colors are emitted as `rgba(r,g,b,a)`.
    fn resolved_color_to_svg_color(color: WorkingColor) -> alloc::string::String {
        let srgb = working::to_srgb(color);
        #[expect(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        let r = (srgb.red * 255.0).clamp(0.0, 255.0) as u8;
        #[expect(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        let g = (srgb.green * 255.0).clamp(0.0, 255.0) as u8;
        #[expect(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        let b = (srgb.blue * 255.0).clamp(0.0, 255.0) as u8;
        let alpha = color.components[3].clamp(0.0, 1.0);
        if alpha >= 0.999_999 {
            alloc::format!("#{r:02x}{g:02x}{b:02x}")
        } else {
            alloc::format!("rgba({r},{g},{b},{alpha:.6})")
        }
    }
}

impl View for Svg {
    fn body(self, env: &Environment) -> impl View {
        if let Some(tint) = self.tint.clone() {
            let color_signal = tint.resolve(env).map(Self::resolved_color_to_svg_color);
            return self.to_reactive_framed_picture(&color_signal);
        }

        if let Some(color_signal) = env
            .query::<waterui_graphics::color::ForegroundColor, Computed<WorkingColor>>()
            .cloned()
        {
            return self
                .to_reactive_framed_picture(&color_signal.map(Self::resolved_color_to_svg_color));
        }

        self.to_framed_picture("#000000")
    }
}
