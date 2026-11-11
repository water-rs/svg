//! SVG rendering for `WaterUI`.
//!
//! This crate provides `Svg`, a view that draws SVG content on a `Canvas`.
//!
//! A document is parsed with `usvg` once and recorded on the canvas as fills,
//! strokes, clips and layers, so it renders on whichever renderer draws the
//! screen around it. The tint is a live paint: a change of the tint repaints
//! the commands that use it and records nothing again. The same commands record
//! into a standalone `Drawing` with [`Svg::drawing`].
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
//! | `text` | off | Enables `usvg/text`, which lays `<text>` out into outline paths, and the font-taking [`Svg::fonts`] setter. Costs `fontdb`, `rustybuzz` and the `unicode-bidi` / `unicode-script` / `unicode-vo` crates. |
//!
//! `usvg` is depended on with `default-features = false`. Its `system-fonts`
//! and `memmap-fonts` defaults are deliberately not enabled: both only widen
//! `fontdb` towards the filesystem (directory and fontconfig scanning,
//! memory-mapped faces), and this crate never asks `fontdb` to read a file. An
//! application that wants the operating system's fonts owns that decision
//! itself — it builds the font database and passes it to [`Svg::fonts`].
//!
//! Without `text`, and with `text` but no fonts, a `<text>` element is dropped
//! during parsing and draws nothing: [`Svg`] is the icon primitive, and icon
//! artwork is paths.

extern crate alloc;

mod document;
mod draw;
mod title;

/// The SVG parser this crate draws from.
pub use usvg;

use alloc::string::String;

use suiteki::Str;
use waterui_core::layout::Size;
use waterui_core::reactive::SignalExt as _;
use waterui_core::{AnyView, Computed, Environment, View, contract_violation};
use waterui_graphics::canvas::{Canvas, Drawing};
use waterui_graphics::color::{Color, ForegroundColor, WorkingColor};
use waterui_layout::frame::Frame;

use crate::document::{Document, fit, path_markup};
use crate::draw::{FIRST_MARKER, SECOND_MARKER};

/// A view for rendering SVG content.
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
    content: Str,
    /// Intrinsic width for aspect ratio.
    width: Option<f32>,
    /// Intrinsic height for aspect ratio.
    height: Option<f32>,
    /// Optional tint color (for monochrome icons).
    tint: Option<Color>,
    /// Whether to render as stroke (outline) rather than fill.
    stroke: bool,
    /// The fonts `<text>` is laid out with.
    #[cfg(feature = "text")]
    fonts: Option<alloc::sync::Arc<usvg::fontdb::Database>>,
}

impl Svg {
    fn with(content: Str, size: Option<(f32, f32)>, stroke: bool) -> Self {
        Self {
            content,
            width: size.map(|(width, _)| width),
            height: size.map(|(_, height)| height),
            tint: None,
            stroke,
            #[cfg(feature = "text")]
            fonts: None,
        }
    }

    /// Creates an SVG from raw SVG markup or path data.
    ///
    /// For icons, prefer `from_path` which provides explicit dimensions.
    pub fn new(content: impl Into<Str>) -> Self {
        Self::with(content.into(), None, false)
    }

    /// Creates an SVG from path data with explicit dimensions.
    ///
    /// This is the recommended constructor for filled icon SVGs where the
    /// path data comes from the `d` attribute of an SVG path element.
    pub fn from_path(path_data: impl Into<Str>, width: f32, height: f32) -> Self {
        Self::with(path_data.into(), Some((width, height)), false)
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
        Self::with(path_data.into(), Some((width, height)), true)
    }

    /// Sets the tint color for the SVG.
    ///
    /// The tint is live: a color that changes repaints the drawing without
    /// recording it again.
    pub fn tint(mut self, color: impl Into<Color>) -> Self {
        self.tint = Some(color.into());
        self
    }

    /// Lays `<text>` out with `fonts`.
    ///
    /// The text becomes outline paths like the rest of the drawing, so it
    /// renders identically on every backend rather than through a platform
    /// text stack. Which fonts exist is the caller's business: this crate never
    /// reads the operating system's font catalogue, so an application that
    /// wants system fonts loads them into the database itself and passes the
    /// result here.
    #[cfg(feature = "text")]
    pub fn fonts(mut self, fonts: alloc::sync::Arc<usvg::fontdb::Database>) -> Self {
        self.fonts = Some(fonts);
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

    /// The SVG markup this view draws, with the colour `marker` where the tint
    /// goes.
    fn markup(&self, marker: &str) -> String {
        let content = self.content.as_str();
        if content.trim_start().starts_with('<') {
            // Full SVG markup sizes itself; the tint reaches what asks for
            // `currentColor`.
            return content.replace("currentColor", marker);
        }
        let (Some(width), Some(height)) = (self.width, self.height) else {
            // Path data without dimensions is passed through as is.
            return content.to_string();
        };
        path_markup(content, width, height, self.stroke, marker)
    }

    /// Parses this SVG, twice: once with each marker colour at the tint, so
    /// that the drawing can tell the tint from the document's own colours.
    fn document(&self) -> Result<Document, usvg::Error> {
        let first = self.markup(FIRST_MARKER);
        let second = self.markup(SECOND_MARKER);
        #[cfg(feature = "text")]
        if let Some(fonts) = &self.fonts {
            return Document::parse_with_fonts(&first, &second, fonts.clone());
        }
        Document::parse(&first, &second)
    }

    /// Records this SVG into a [`Drawing`] of its own size, painted with `tint`
    /// where it asks for the tint.
    ///
    /// A drawing is constants, so a tint that should follow a signal draws the
    /// `Svg` view instead.
    ///
    /// # Errors
    ///
    /// When the markup is not an SVG document, a zero or missing size
    /// included.
    pub fn drawing(&self, tint: WorkingColor) -> Result<Drawing, usvg::Error> {
        let document = self.document()?;
        Ok(Drawing::record(document.intrinsic_size(), |cx| {
            draw::render(cx, &document, &tint);
        }))
    }

    /// The name the document gives itself: the root `<title>` of full markup.
    ///
    /// Path data carries no title, so an icon built from a `d` attribute stays
    /// unnamed until the application names it with `.a11y_label(…)` — which
    /// also wins over a title wherever both exist.
    fn accessible_name(&self) -> Option<Str> {
        let content = self.content.as_str();
        content
            .trim_start()
            .starts_with('<')
            .then(|| title::document_title(content))
            .flatten()
            .map(Str::from)
    }

    /// The colour the SVG is tinted with: its own tint, else the foreground
    /// colour of the environment, else black.
    fn tint_in(&self, env: &Environment) -> Computed<WorkingColor> {
        self.tint
            .as_ref()
            .map(|color| color.resolve(env))
            .or_else(|| {
                env.query::<ForegroundColor, Computed<WorkingColor>>()
                    .cloned()
            })
            .unwrap_or_else(|| Computed::constant(WorkingColor::BLACK))
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
    /// canvas the frame wraps draws into whatever extent it is proposed — that
    /// is right for a `Canvas`, and it is why an icon needs the ceiling said
    /// out loud. Without it a stack that proposes its row height (`HStack`
    /// proposes the bounds height to a child that does not stretch) would get
    /// an icon stretched to that height. There is deliberately no *minimum*:
    /// the drawing stays free to shrink into a smaller box.
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
}

impl View for Svg {
    fn body(self, env: &Environment) -> impl View {
        let document = match self.document() {
            Ok(document) => document,
            Err(error) => {
                contract_violation!("{error}; the Svg draws nothing");
                return AnyView::new(());
            }
        };
        let tint = self.tint_in(env);
        let natural = document.intrinsic_size();
        let label = self.accessible_name();

        let canvas = Canvas::new(move |cx| {
            // The document is recorded once, in its own coordinates; the box
            // layout gives the canvas moves the whole drawing, not its
            // commands, and the tint reaches only the paints that use it.
            let placement = cx.size().map(move |size: Size| fit(natural, size));
            cx.transform(placement, |cx| draw::render(cx, &document, &tint));
        })
        .intrinsic_size(Computed::constant(Some(natural)))
        .offered_label(Computed::constant(label));

        AnyView::new(self.frame_view(canvas))
    }
}

#[cfg(test)]
mod tests {
    use super::Svg;

    const TITLED_ICON: &str = include_str!("../tests/data/titled_icon.svg");
    const PAINTED_ICON: &str = include_str!("../tests/data/painted_icon.svg");

    /// A document's own `<title>` becomes the label its canvas offers a screen
    /// reader; an application's `a11y_label` wins over it when the view
    /// mounts, which is the mounted-runtime half of this check.
    #[test]
    fn a_titled_document_offers_its_title_as_the_canvass_label() {
        assert_eq!(
            Svg::new(TITLED_ICON)
                .accessible_name()
                .as_ref()
                .map(suiteki::Str::as_str),
            Some("Warning sign")
        );
    }

    /// Path data and a document with no `<title>` offer nothing.
    #[test]
    fn an_untitled_drawing_offers_no_label() {
        assert_eq!(
            Svg::from_path("M3 12h18M3 6h18", 24.0, 24.0).accessible_name(),
            None
        );
        assert_eq!(Svg::new(PAINTED_ICON).accessible_name(), None);
    }
}
