//! Parsed SVG documents, ready to draw on a canvas.

use alloc::string::String;

use waterui_core::layout::{Affine2, Size};

/// A parsed SVG document.
///
/// It is parsed twice, from markup that differs only in the colour at the
/// tint: the two trees share a structure, and a colour is the tint exactly
/// where they differ.
#[derive(Debug)]
pub struct Document {
    first: usvg::Tree,
    second: usvg::Tree,
}

impl Document {
    /// Parses the two markups of one document.
    ///
    /// The parser is given no fonts, so a `<text>` element is laid out against
    /// an empty font database and contributes nothing to the drawing. That is
    /// what an icon wants — the artwork is geometry, and it looks the same on
    /// every backend and every device. To draw a document whose text matters,
    /// hand the fonts in with [`Document::parse_with_fonts`].
    pub fn parse(first: &str, second: &str) -> Self {
        Self::parse_with_options(first, second, &usvg::Options::default())
    }

    /// Parses the two markups of one document, laying `<text>` out with
    /// `fonts`.
    ///
    /// The text becomes outline paths like the rest of the drawing, so it
    /// renders identically on every backend rather than through a platform
    /// text stack. Which fonts exist is the caller's business: this crate never
    /// reads the operating system's font catalogue, so an application that
    /// wants system fonts loads them into the database itself and passes the
    /// result here.
    #[cfg(feature = "text")]
    pub fn parse_with_fonts(
        first: &str,
        second: &str,
        fonts: alloc::sync::Arc<usvg::fontdb::Database>,
    ) -> Self {
        Self::parse_with_options(
            first,
            second,
            &usvg::Options {
                fontdb: fonts,
                ..usvg::Options::default()
            },
        )
    }

    fn parse_with_options(first: &str, second: &str, options: &usvg::Options<'_>) -> Self {
        let parse = |markup: &str| {
            let tree = usvg::Tree::from_str(markup, options).expect("failed to parse SVG content");
            let size = tree.size();
            assert!(
                size.width().is_finite()
                    && size.height().is_finite()
                    && size.width() > 0.0
                    && size.height() > 0.0,
                "SVG must have positive finite dimensions, got {}x{}",
                size.width(),
                size.height()
            );
            tree
        };
        Self {
            first: parse(first),
            second: parse(second),
        }
    }

    /// The tree parsed with the first marker colour.
    pub(crate) const fn first(&self) -> &usvg::Tree {
        &self.first
    }

    /// The tree parsed with the second marker colour.
    pub(crate) const fn second(&self) -> &usvg::Tree {
        &self.second
    }

    /// The document's own size, from its `viewBox` / `width` / `height`.
    ///
    /// This is the size an SVG *is*: `parse` has already established it is finite
    /// and positive, so an icon with no frame around it measures at the size its
    /// author drew, and one given a single axis keeps this document's aspect ratio.
    pub fn intrinsic_size(&self) -> Size {
        let size = self.first.size();
        Size::new(size.width(), size.height())
    }
}

/// The transform that fits a document of size `natural` into a box of size
/// `into`.
///
/// Aspect ratio is preserved and the result centred, which is what an icon in
/// a fixed-size slot wants.
pub fn fit(natural: Size, into: Size) -> Affine2 {
    let scale = (into.width / natural.width).min(into.height / natural.height);
    let offset_x = natural.width.mul_add(-scale, into.width) / 2.0;
    let offset_y = natural.height.mul_add(-scale, into.height) / 2.0;
    Affine2::new(scale, 0.0, 0.0, scale, offset_x, offset_y)
}

/// The markup of an SVG built from path data, in which `marker` is the fill
/// or stroke colour.
pub fn path_markup(path_data: &str, width: f32, height: f32, stroke: bool, marker: &str) -> String {
    if stroke {
        // Stroke-based path (for outline icons like Lucide).
        alloc::format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" fill="none" stroke="{marker}" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="{path_data}"/></svg>"#
        )
    } else {
        // Filled path data, wrapped in an SVG document.
        alloc::format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" fill="{marker}"><path d="{path_data}"/></svg>"#
        )
    }
}

#[cfg(test)]
mod tests {
    use super::Document;
    use crate::usvg;

    const GENERIC_FAMILY: &str = include_str!("../tests/data/text_generic_family.svg");
    const NAMED_FAMILY: &str = include_str!("../tests/data/text_named_family.svg");
    const PAINTED_ICON: &str = include_str!("../tests/data/painted_icon.svg");
    const STROKED_ICON: &str = include_str!("../tests/data/stroked_icon.svg");
    /// The one font the text fixtures are laid out with, so that they flatten
    /// into the same outlines on every machine.
    #[cfg(feature = "text")]
    const ROBOTO: &[u8] = include_bytes!("../tests/fonts/Roboto-Regular.ttf");

    fn parse(content: &str) -> Document {
        Document::parse(content, content)
    }

    /// What a parsed document actually contains, counted node by node.
    #[derive(Debug, Default, PartialEq, Eq)]
    struct Census {
        paths: usize,
        /// Path segments outside `<text>`, which is where a change in the
        /// drawing itself would show up.
        segments: usize,
        texts: usize,
        text_outlines: usize,
        text_outline_segments: usize,
    }

    impl Census {
        fn of(document: &Document) -> Self {
            let mut census = Self::default();
            census.visit(document.first().root());
            census
        }

        fn visit(&mut self, group: &usvg::Group) {
            for node in group.children() {
                match node {
                    usvg::Node::Group(group) => self.visit(group),
                    usvg::Node::Path(path) => {
                        self.paths += 1;
                        self.segments += path.data().len();
                    }
                    usvg::Node::Image(_) => {}
                    usvg::Node::Text(text) => {
                        self.texts += 1;
                        let (paths, segments) = (self.paths, self.segments);
                        self.visit(text.flattened());
                        self.text_outlines += self.paths - paths;
                        self.text_outline_segments += self.segments - segments;
                        self.paths = paths;
                        self.segments = segments;
                    }
                }
            }
        }
    }

    /// Geometry is what this crate is for, and no font policy touches it.
    #[test]
    fn geometry_parses_the_same_whatever_the_fonts() {
        assert_eq!(
            Census::of(&parse(PAINTED_ICON)),
            Census {
                paths: 4,
                segments: 25,
                texts: 0,
                text_outlines: 0,
                text_outline_segments: 0,
            }
        );
        assert_eq!(
            Census::of(&parse(STROKED_ICON)),
            Census {
                paths: 2,
                segments: 12,
                texts: 0,
                text_outlines: 0,
                text_outline_segments: 0,
            }
        );
    }

    /// `<text>` needs a font, and [`Document::parse`] supplies none — so the
    /// element is dropped during parsing and only the background rectangle of
    /// each fixture survives.
    ///
    /// This holds for a generic family and for a family installed on the
    /// machine alike, which is the evidence that `usvg`'s `system-fonts` and
    /// `memmap-fonts` features had nothing to contribute: the database they
    /// widen is never consulted, because it is never populated.
    #[test]
    fn text_without_fonts_draws_nothing() {
        for (fixture, content) in [("generic", GENERIC_FAMILY), ("named", NAMED_FAMILY)] {
            assert_eq!(
                Census::of(&parse(content)),
                Census {
                    paths: 1,
                    segments: 5,
                    texts: 0,
                    text_outlines: 0,
                    text_outline_segments: 0,
                },
                "{fixture} family fixture should keep only its background rectangle"
            );
        }
    }

    /// Handed a font database, the same fixtures lay out into outline paths.
    ///
    /// The database holds one font, bundled with these tests, and it is built
    /// from bytes: no filesystem scanning, no fontconfig, none of what `usvg`'s
    /// `system-fonts` feature would have pulled into the library. That is the
    /// evidence the feature is not needed — and it also keeps the assertion
    /// about this crate rather than about whichever fonts a machine happens to
    /// have installed.
    #[cfg(feature = "text")]
    #[test]
    fn text_with_fonts_becomes_outline_paths() {
        let mut fonts = usvg::fontdb::Database::new();
        fonts.load_font_data(ROBOTO.to_vec());
        fonts.set_sans_serif_family("Roboto");
        let fonts = alloc::sync::Arc::new(fonts);

        for (fixture, content) in [("generic", GENERIC_FAMILY), ("named", NAMED_FAMILY)] {
            let census = Census::of(&Document::parse_with_fonts(content, content, fonts.clone()));
            assert_eq!(census.texts, 1, "{fixture} family fixture should lay out");
            assert!(
                census.text_outlines >= 1 && census.text_outline_segments >= 20,
                "{fixture} family fixture should flatten 'Water' into glyph outlines, got {} path(s) of {} segment(s)",
                census.text_outlines,
                census.text_outline_segments
            );
            assert_eq!(census.paths, 1, "the background rectangle should survive");
        }
    }
}
