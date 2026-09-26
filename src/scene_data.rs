//! Parsed SVG documents, ready to draw into a scene.

use cherenkov::kurbo::Affine;
use waterui_core::layout::Size;

use crate::tree_renderer::SvgTarget;

/// A parsed SVG document.
#[derive(Debug)]
pub struct SvgSceneData {
    svg_tree: usvg::Tree,
}

impl SvgSceneData {
    /// Parses SVG content.
    ///
    /// The parser is given no fonts, so a `<text>` element is laid out against
    /// an empty font database and contributes nothing to the drawing. That is
    /// what an icon wants — the artwork is geometry, and it looks the same on
    /// every backend and every device. To draw a document whose text matters,
    /// hand the fonts in with [`SvgSceneData::parse_with_fonts`].
    #[must_use]
    pub fn parse(svg_content: &str) -> Self {
        Self::parse_with_options(svg_content, &usvg::Options::default())
    }

    /// Parses SVG content, laying `<text>` out with `fonts`.
    ///
    /// The text becomes outline paths like the rest of the drawing, so it
    /// renders identically on every backend rather than through a platform
    /// text stack. Which fonts exist is the caller's business: this crate never
    /// reads the operating system's font catalogue, so an application that
    /// wants system fonts loads them into the database itself and passes the
    /// result here.
    #[cfg(feature = "text")]
    #[must_use]
    pub fn parse_with_fonts(
        svg_content: &str,
        fonts: alloc::sync::Arc<usvg::fontdb::Database>,
    ) -> Self {
        Self::parse_with_options(
            svg_content,
            &usvg::Options {
                fontdb: fonts,
                ..usvg::Options::default()
            },
        )
    }

    fn parse_with_options(svg_content: &str, options: &usvg::Options<'_>) -> Self {
        let svg_tree =
            usvg::Tree::from_str(svg_content, options).expect("failed to parse SVG content");
        let svg_size = svg_tree.size();
        assert!(
            svg_size.width().is_finite()
                && svg_size.height().is_finite()
                && svg_size.width() > 0.0
                && svg_size.height() > 0.0,
            "SVG must have positive finite dimensions, got {}x{}",
            svg_size.width(),
            svg_size.height()
        );

        Self { svg_tree }
    }

    /// The document's own size, from its `viewBox` / `width` / `height`.
    ///
    /// This is the size an SVG *is*: `parse` has already established it is finite
    /// and positive, so an icon with no frame around it measures at the size its
    /// author drew, and one given a single axis keeps this document's aspect ratio.
    #[must_use]
    pub fn intrinsic_size(&self) -> Size {
        let size = self.svg_tree.size();
        Size::new(size.width(), size.height())
    }

    /// The transform that fits this document into `width` × `height`.
    ///
    /// Aspect ratio is preserved and the result centred, which is what an icon
    /// in a fixed-size slot wants.
    #[must_use]
    pub fn fitting_transform(&self, width: f32, height: f32) -> Affine {
        let size = self.svg_tree.size();
        let scale = (width / size.width()).min(height / size.height());
        let offset_x = f64::from(size.width().mul_add(-scale, width) / 2.0);
        let offset_y = f64::from(size.height().mul_add(-scale, height) / 2.0);
        Affine::translate((offset_x, offset_y)) * Affine::scale(f64::from(scale))
    }

    /// Records this document into `scene`, fitted into `width` × `height`.
    ///
    /// `scene` is whichever Cherenkov recorder is listening — a static picture
    /// or a live scene — so an icon renders on every backend rather than
    /// through one engine's scene type.
    pub fn draw<D: SvgTarget>(&self, scene: &mut D, width: f32, height: f32) {
        crate::tree_renderer::render_tree(
            scene,
            &self.svg_tree,
            self.fitting_transform(width, height),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::SvgSceneData;
    use crate::usvg;

    const GENERIC_FAMILY: &str = include_str!("../tests/data/text_generic_family.svg");
    const NAMED_FAMILY: &str = include_str!("../tests/data/text_named_family.svg");
    const PAINTED_ICON: &str = include_str!("../tests/data/painted_icon.svg");
    const STROKED_ICON: &str = include_str!("../tests/data/stroked_icon.svg");
    /// The one font the text fixtures are laid out with, so that they flatten
    /// into the same outlines on every machine.
    #[cfg(feature = "text")]
    const ROBOTO: &[u8] = include_bytes!("../tests/fonts/Roboto-Regular.ttf");

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
        fn of(data: &SvgSceneData) -> Self {
            let mut census = Self::default();
            census.visit(data.svg_tree.root());
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
            Census::of(&SvgSceneData::parse(PAINTED_ICON)),
            Census {
                paths: 4,
                segments: 25,
                texts: 0,
                text_outlines: 0,
                text_outline_segments: 0,
            }
        );
        assert_eq!(
            Census::of(&SvgSceneData::parse(STROKED_ICON)),
            Census {
                paths: 2,
                segments: 12,
                texts: 0,
                text_outlines: 0,
                text_outline_segments: 0,
            }
        );
    }

    /// `<text>` needs a font, and [`SvgSceneData::parse`] supplies none — so the
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
                Census::of(&SvgSceneData::parse(content)),
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
            let census = Census::of(&SvgSceneData::parse_with_fonts(content, fonts.clone()));
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
