//! Draws a parsed SVG document on a canvas.
//!
//! `vello_svg` renders into a `vello::Scene` and nothing else, which ties every
//! icon in an application to one rendering engine. Recording onto a
//! [`waterui_graphics::canvas`] surface instead lets the same document render
//! on whichever renderer draws the screen around it, and the same commands
//! record into a standalone [`Drawing`](waterui_graphics::canvas::Drawing).
//!
//! The translation follows `vello_svg`'s own renderer, which is where the
//! handling of paint order, clip paths, nested documents and flattened text
//! comes from.
//!
//! # The tint
//!
//! An SVG is drawn in its own colours except where it asks for the colour the
//! surrounding view is drawn in: the icon's `currentColor`, or the fill or
//! stroke of a path-data icon. The parser resolves those to a colour like any
//! other, so the document is parsed twice, once with each of two different
//! marker colours standing in for the tint, and a paint, gradient stop or
//! stroke colour is the tint exactly where the two parses disagree on it. A
//! colour the document spells itself is the same in both, whatever it is, and
//! is never mistaken for the tint. Every paint that holds the tint is built
//! from it instead. On a live [`Cx`] such a paint is a signal of the tint, so a change
//! of the tint repaints the commands that use it and records nothing again; on
//! a `Drawing` it is the constant the drawing was recorded with.
//!
//! One SVG semantic the canvas cannot express: SVG composites in sRGB-encoded
//! space (`color-interpolation` defaults to sRGB), while the renderers
//! composite in a linear working space. `Layer::blend_space` could mark a
//! composite as encoded, but the GPU backend rejects any non-linear blend space
//! outright, so recording one would make a document fail to render there. Gradient
//! stops are interpolated in encoded space, which every renderer takes.

use alloc::rc::Rc;
use alloc::vec::Vec;
use core::cell::Cell;

use waterui_core::layout::{Affine2, Point};
use waterui_core::reactive::{Computed, SignalExt as _};
use waterui_graphics::canvas::{
    BlendMode, Cap, ColorStop, Cx, DrawingCx, Extend, FillRule, Interpolation, Join, Layer,
    LinearGradient, Paint, Path, RadialGradient, StrokeStyle,
};
use waterui_graphics::color::{WorkingColor, srgb_to_linear, working};

use crate::document::Document;
use crate::usvg;

/// The colour the first parse of a document has where the tint belongs.
pub const FIRST_MARKER: &str = "#ff0000";

/// The colour the second parse of a document has where the tint belongs.
pub const SECOND_MARKER: &str = "#0000ff";

/// The same node in the two parses of a document.
///
/// The parses differ only in the colour at the tint, so their trees share a
/// structure node for node, and a colour is the tint exactly where the two
/// differ.
struct Twin<'a, T> {
    first: &'a T,
    second: &'a T,
}

impl<T> Clone for Twin<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Twin<'_, T> {}

impl<'a, T> Twin<'a, T> {
    const fn new(first: &'a T, second: &'a T) -> Self {
        Self { first, second }
    }
}

/// What a fill or stroke is painted with: a paint the document fixes, or one
/// built from the tint.
#[derive(Clone)]
pub enum Ink {
    /// A paint that is the same whatever the tint.
    Fixed(Paint),
    /// A paint built from the tint.
    Tinted(Rc<dyn Fn(WorkingColor) -> Paint>),
}

impl Ink {
    /// The paint when the tint is `tint`.
    fn with(&self, tint: WorkingColor) -> Paint {
        match self {
            Self::Fixed(paint) => paint.clone(),
            Self::Tinted(build) => build(tint),
        }
    }

    /// The paint as a signal that follows `tint`, so a change of the tint
    /// changes the paint in place.
    fn live(&self, tint: &Computed<WorkingColor>) -> Computed<Paint> {
        match self {
            Self::Fixed(paint) => Computed::constant(paint.clone()),
            Self::Tinted(build) => {
                let build = Rc::clone(build);
                tint.map(move |color| build(color)).computed()
            }
        }
    }
}

/// What a document is drawn on: a live [`Cx`], whose tint is a signal, or a
/// [`DrawingCx`], whose tint is a constant.
pub trait Surface: Sized {
    /// What the tint is on this surface.
    type Tint;

    /// Fills `path` with `ink`.
    fn paint_fill(&mut self, path: Path, ink: &Ink, tint: &Self::Tint);

    /// Strokes `path` in `style` with `ink`.
    fn paint_stroke(&mut self, path: Path, style: StrokeStyle, ink: &Ink, tint: &Self::Tint);

    /// Runs `body` under `transform`.
    fn placed(&mut self, transform: Affine2, body: impl FnOnce(&mut Self));

    /// Runs `body` clipped to `path`.
    fn clipped(&mut self, path: Path, body: impl FnOnce(&mut Self));

    /// Runs `body` isolated in `layer`.
    fn layered(&mut self, layer: Layer, body: impl FnOnce(&mut Self));
}

impl Surface for Cx {
    type Tint = Computed<WorkingColor>;

    fn paint_fill(&mut self, path: Path, ink: &Ink, tint: &Self::Tint) {
        Self::fill(self, path, ink.live(tint));
    }

    fn paint_stroke(&mut self, path: Path, style: StrokeStyle, ink: &Ink, tint: &Self::Tint) {
        Self::stroke(self, path, style, ink.live(tint));
    }

    fn placed(&mut self, transform: Affine2, body: impl FnOnce(&mut Self)) {
        self.transform(transform, body);
    }

    fn clipped(&mut self, path: Path, body: impl FnOnce(&mut Self)) {
        self.clip(path, body);
    }

    fn layered(&mut self, layer: Layer, body: impl FnOnce(&mut Self)) {
        self.layer(layer, body);
    }
}

impl Surface for DrawingCx {
    type Tint = WorkingColor;

    fn paint_fill(&mut self, path: Path, ink: &Ink, tint: &Self::Tint) {
        Self::fill(self, path, ink.with(*tint));
    }

    fn paint_stroke(&mut self, path: Path, style: StrokeStyle, ink: &Ink, tint: &Self::Tint) {
        Self::stroke(self, path, &style, ink.with(*tint));
    }

    fn placed(&mut self, transform: Affine2, body: impl FnOnce(&mut Self)) {
        self.transform(transform, body);
    }

    fn clipped(&mut self, path: Path, body: impl FnOnce(&mut Self)) {
        self.clip(path, body);
    }

    fn layered(&mut self, layer: Layer, body: impl FnOnce(&mut Self)) {
        self.layer(layer, body);
    }
}

/// What drawing a document carries down the tree.
struct Context<'a, T> {
    tint: &'a T,
}

/// Draws a whole document on `surface`, its own coordinates at the surface's
/// origin, painted with `tint` where it asks for the tint.
pub fn render<S: Surface>(surface: &mut S, document: &Document, tint: &S::Tint) {
    let context = Context { tint };
    render_group(
        surface,
        Twin::new(document.first().root(), document.second().root()),
        &context,
    );
}

/// Draws a group's children.
///
/// Every node carries its own *absolute* transform, so each is placed by its
/// own transform from the origin of the surface and nothing is composed down
/// the tree.
fn render_group<S: Surface>(
    surface: &mut S,
    group: Twin<'_, usvg::Group>,
    context: &Context<'_, S::Tint>,
) {
    debug_assert_eq!(
        group.first.children().len(),
        group.second.children().len(),
        "the two parses of a document share a structure"
    );
    for (first, second) in group.first.children().iter().zip(group.second.children()) {
        match (first, second) {
            (usvg::Node::Group(first), usvg::Node::Group(second)) => {
                render_nested_group(surface, Twin::new(first, second), context);
            }
            (usvg::Node::Path(first), usvg::Node::Path(second)) => {
                render_path(surface, Twin::new(first, second), context);
            }
            (usvg::Node::Image(first), usvg::Node::Image(second)) => {
                render_image(surface, Twin::new(first, second), context);
            }
            // Text arrives already flattened into outlines, so this draws
            // glyphs the same way it draws any other geometry. The parser only
            // produces such a node when it had both the `text` feature and a
            // font to lay the text out with; otherwise it drops the `<text>`
            // element and this arm never runs.
            (usvg::Node::Text(first), usvg::Node::Text(second)) => render_group(
                surface,
                Twin::new(first.flattened(), second.flattened()),
                context,
            ),
            _ => unreachable!("the two parses of a document share a structure"),
        }
    }
}

fn render_nested_group<S: Surface>(
    surface: &mut S,
    group: Twin<'_, usvg::Group>,
    context: &Context<'_, S::Tint>,
) {
    let tree = group;
    let group = group.first;
    let alpha = group.opacity().get();
    let blend = to_blend_mode(group.blend_mode());

    // A clip path with a single path clips to it; anything else clips to the
    // group's bounding box, which is what `vello_svg` does. The clip element
    // sits in the node's absolute placement, so the clip shape is carried into
    // that space here. A group without a clip path draws its children
    // unclipped — the bounding-box fallback never cropped anything.
    let clip = group.clip_path().map(|clip_path| {
        let shape = clip_path
            .root()
            .children()
            .first()
            .and_then(|node| match node {
                usvg::Node::Path(path) => Some(path.data().clone()),
                _ => None,
            })
            .unwrap_or_else(|| {
                usvg::tiny_skia_path::PathBuilder::from_rect(group.layer_bounding_box().to_rect())
            });
        to_path(
            &shape
                .transform(group.abs_transform())
                .expect("a clip path stays finite under its node's transform"),
            FillRule::NonZero,
        )
    });

    let isolated = group.opacity() != usvg::NormalizedF32::ONE || blend != BlendMode::Normal;
    match (clip, isolated) {
        (Some(clip), true) => {
            surface.layered(Layer::opacity(alpha).blend(blend), |surface| {
                surface.clipped(clip, |surface| render_group(surface, tree, context));
            });
        }
        (Some(clip), false) => {
            surface.clipped(clip, |surface| render_group(surface, tree, context));
        }
        (None, true) => {
            surface.layered(Layer::opacity(alpha).blend(blend), |surface| {
                render_group(surface, tree, context);
            });
        }
        (None, false) => render_group(surface, tree, context),
    }
}

/// Runs `body` placed by `transform`, which is left out when it places nothing.
fn placed<S: Surface>(surface: &mut S, transform: &usvg::Transform, body: impl FnOnce(&mut S)) {
    if transform.is_identity() {
        body(surface);
    } else {
        surface.placed(to_affine(transform), body);
    }
}

fn render_path<S: Surface>(
    surface: &mut S,
    twin: Twin<'_, usvg::Path>,
    context: &Context<'_, S::Tint>,
) {
    let path = twin.first;
    if !path.is_visible() {
        return;
    }

    let fill = |surface: &mut S| {
        if let Some(fill) = path.fill()
            && let Some(second) = twin.second.fill()
            && let Some(ink) = to_ink(Twin::new(fill.paint(), second.paint()), fill.opacity())
        {
            let rule = match fill.rule() {
                usvg::FillRule::NonZero => FillRule::NonZero,
                usvg::FillRule::EvenOdd => FillRule::EvenOdd,
            };
            surface.paint_fill(to_path(path.data(), rule), &ink, context.tint);
        }
    };
    let stroke = |surface: &mut S| {
        if let Some(stroke) = path.stroke()
            && let Some(second) = twin.second.stroke()
            && let Some(ink) = to_ink(Twin::new(stroke.paint(), second.paint()), stroke.opacity())
        {
            surface.paint_stroke(
                to_path(path.data(), FillRule::NonZero),
                to_stroke(stroke),
                &ink,
                context.tint,
            );
        }
    };

    placed(surface, &path.abs_transform(), |surface| {
        match path.paint_order() {
            usvg::PaintOrder::FillAndStroke => {
                fill(surface);
                stroke(surface);
            }
            usvg::PaintOrder::StrokeAndFill => {
                stroke(surface);
                fill(surface);
            }
        }
    });
}

fn render_image<S: Surface>(
    surface: &mut S,
    twin: Twin<'_, usvg::Image>,
    context: &Context<'_, S::Tint>,
) {
    let image = twin.first;
    if !image.is_visible() {
        return;
    }
    match image.kind() {
        // A nested document is placed by this node, and its own nodes carry
        // absolute transforms within it.
        // It is parsed from its own data, which carries no marker, so it is
        // the same in both parses and nothing in it is the tint.
        usvg::ImageKind::SVG(svg) => placed(surface, &image.abs_transform(), |surface| {
            render_group(surface, Twin::new(svg.root(), svg.root()), context);
        }),
        // Raster images inside an SVG are not decoded here. An icon set does not
        // use them, and pulling an image decoder into every application that
        // draws an icon is not worth the one case that would.
        _ => {
            tracing::debug!(
                target: "waterui::svg",
                "Raster image inside an SVG is not drawn"
            );
        }
    }
}

const fn to_blend_mode(mode: usvg::BlendMode) -> BlendMode {
    match mode {
        usvg::BlendMode::Normal => BlendMode::Normal,
        usvg::BlendMode::Multiply => BlendMode::Multiply,
        usvg::BlendMode::Screen => BlendMode::Screen,
        usvg::BlendMode::Overlay => BlendMode::Overlay,
        usvg::BlendMode::Darken => BlendMode::Darken,
        usvg::BlendMode::Lighten => BlendMode::Lighten,
        usvg::BlendMode::ColorDodge => BlendMode::ColorDodge,
        usvg::BlendMode::ColorBurn => BlendMode::ColorBurn,
        usvg::BlendMode::HardLight => BlendMode::HardLight,
        usvg::BlendMode::SoftLight => BlendMode::SoftLight,
        usvg::BlendMode::Difference => BlendMode::Difference,
        usvg::BlendMode::Exclusion => BlendMode::Exclusion,
        usvg::BlendMode::Hue => BlendMode::Hue,
        usvg::BlendMode::Saturation => BlendMode::Saturation,
        usvg::BlendMode::Color => BlendMode::Color,
        usvg::BlendMode::Luminosity => BlendMode::Luminosity,
    }
}

const fn to_affine(transform: &usvg::Transform) -> Affine2 {
    Affine2::new(
        transform.sx,
        transform.ky,
        transform.kx,
        transform.sy,
        transform.tx,
        transform.ty,
    )
}

const fn point(x: f32, y: f32) -> Point {
    Point::new(x, y)
}

/// The canvas path of a parsed outline, enclosing by `rule`.
fn to_path(data: &usvg::tiny_skia_path::Path, rule: FillRule) -> Path {
    data.segments()
        .fold(
            Path::builder().fill_rule(rule),
            |builder, segment| match segment {
                usvg::tiny_skia_path::PathSegment::MoveTo(to) => builder.move_to(point(to.x, to.y)),
                usvg::tiny_skia_path::PathSegment::LineTo(to) => builder.line_to(point(to.x, to.y)),
                usvg::tiny_skia_path::PathSegment::QuadTo(control, to) => {
                    builder.quad_to(point(control.x, control.y), point(to.x, to.y))
                }
                usvg::tiny_skia_path::PathSegment::CubicTo(first, second, to) => builder.cubic_to(
                    point(first.x, first.y),
                    point(second.x, second.y),
                    point(to.x, to.y),
                ),
                usvg::tiny_skia_path::PathSegment::Close => builder.close(),
            },
        )
        .build()
}

fn to_stroke(stroke: &usvg::Stroke) -> StrokeStyle {
    StrokeStyle::new(stroke.width().get())
        .cap(match stroke.linecap() {
            usvg::LineCap::Butt => Cap::Butt,
            usvg::LineCap::Round => Cap::Round,
            usvg::LineCap::Square => Cap::Square,
        })
        .join(match stroke.linejoin() {
            usvg::LineJoin::Miter | usvg::LineJoin::MiterClip => Join::Miter,
            usvg::LineJoin::Round => Join::Round,
            usvg::LineJoin::Bevel => Join::Bevel,
        })
        .miter_limit(stroke.miterlimit().get())
        .dash(
            stroke.dasharray().map_or_else(Vec::new, <[f32]>::to_vec),
            stroke.dashoffset(),
        )
}

/// What a fill or stroke paints with: fixed when the document alone decides
/// it, built from the tint when any colour in it is the tint.
///
/// A pattern paints with a whole subtree rather than a paint, which no canvas
/// command expresses; such a shape is left undrawn rather than painted a wrong
/// colour.
fn to_ink(paint: Twin<'_, usvg::Paint>, opacity: usvg::Opacity) -> Option<Ink> {
    let tinted = Cell::new(false);
    let Some(fixed) = build(paint, opacity, WorkingColor::BLACK, &tinted) else {
        tracing::debug!(
            target: "waterui::svg",
            "A pattern paint inside an SVG is not drawn"
        );
        return None;
    };
    if !tinted.get() {
        return Some(Ink::Fixed(fixed));
    }
    let (first, second) = (paint.first.clone(), paint.second.clone());
    Some(Ink::Tinted(Rc::new(move |tint| {
        build(Twin::new(&first, &second), opacity, tint, &Cell::new(false))
            .expect("a paint that built once builds again")
    })))
}

/// The paint `paint` is when the tint is `tint`, noting in `tinted` whether any
/// of its colours was the tint: a colour is, exactly where the two parses of
/// the document disagree on it.
fn build(
    paint: Twin<'_, usvg::Paint>,
    opacity: usvg::Opacity,
    tint: WorkingColor,
    tinted: &Cell<bool>,
) -> Option<Paint> {
    let opacity = opacity.get();
    let color = |colors: Twin<'_, usvg::Color>, alpha: f32| {
        let color = *colors.first;
        if colors.first == colors.second {
            working::from_linear_srgb(
                [color.red, color.green, color.blue]
                    .map(|channel| srgb_to_linear(f32::from(channel) / 255.0)),
                alpha,
            )
        } else {
            tinted.set(true);
            let [red, green, blue, tint_alpha] = tint.components;
            WorkingColor::new([red, green, blue, tint_alpha * alpha])
        }
    };
    let stops = |first: &[usvg::Stop], second: &[usvg::Stop]| -> Vec<ColorStop> {
        first
            .iter()
            .zip(second)
            .map(|(stop, other)| ColorStop {
                offset: stop.offset().get(),
                color: color(
                    Twin::new(&stop.color(), &other.color()),
                    stop.opacity().get() * opacity,
                ),
            })
            .collect()
    };
    match (paint.first, paint.second) {
        (usvg::Paint::Color(solid), usvg::Paint::Color(other)) => {
            Some(Paint::Color(color(Twin::new(solid, other), opacity)))
        }
        (usvg::Paint::LinearGradient(gradient), usvg::Paint::LinearGradient(other)) => {
            let mut linear = LinearGradient::new(
                point(gradient.x1(), gradient.y1()),
                point(gradient.x2(), gradient.y2()),
            )
            .extend(to_extend(gradient.spread_method()))
            // SVG gradients interpolate in sRGB.
            .interpolation(Interpolation::Encoded);
            linear.stops = stops(gradient.stops(), other.stops());
            Some(Paint::Linear(linear).transformed(to_affine(&gradient.transform())))
        }
        (usvg::Paint::RadialGradient(gradient), usvg::Paint::RadialGradient(other)) => {
            // The focal point has no radius of its own in this SVG model.
            let mut radial = RadialGradient::two_point(
                point(gradient.fx(), gradient.fy()),
                0.0,
                point(gradient.cx(), gradient.cy()),
                gradient.r().get(),
            )
            .extend(to_extend(gradient.spread_method()))
            .interpolation(Interpolation::Encoded);
            radial.stops = stops(gradient.stops(), other.stops());
            Some(Paint::Radial(radial).transformed(to_affine(&gradient.transform())))
        }
        (usvg::Paint::Pattern(_), _) => None,
        _ => unreachable!("the two parses of a document share a structure"),
    }
}

const fn to_extend(spread: usvg::SpreadMethod) -> Extend {
    match spread {
        usvg::SpreadMethod::Pad => Extend::Pad,
        usvg::SpreadMethod::Reflect => Extend::Reflect,
        usvg::SpreadMethod::Repeat => Extend::Repeat,
    }
}

#[cfg(test)]
mod tests {
    use waterui_core::reactive::{Signal as _, binding};

    use super::*;
    use crate::Svg;

    /// A surface that keeps what is drawn on it, in the way a live canvas
    /// does: each paint is the signal of the tint.
    #[derive(Default)]
    struct Recording {
        commands: usize,
        paints: Vec<Computed<Paint>>,
    }

    impl Surface for Recording {
        type Tint = Computed<WorkingColor>;

        fn paint_fill(&mut self, _path: Path, ink: &Ink, tint: &Self::Tint) {
            self.commands += 1;
            self.paints.push(ink.live(tint));
        }

        fn paint_stroke(&mut self, _: Path, _: StrokeStyle, ink: &Ink, tint: &Self::Tint) {
            self.commands += 1;
            self.paints.push(ink.live(tint));
        }

        fn placed(&mut self, _: Affine2, body: impl FnOnce(&mut Self)) {
            self.commands += 1;
            body(self);
        }

        fn clipped(&mut self, _: Path, body: impl FnOnce(&mut Self)) {
            self.commands += 1;
            body(self);
        }

        fn layered(&mut self, _: Layer, body: impl FnOnce(&mut Self)) {
            self.commands += 1;
            body(self);
        }
    }

    const RED: WorkingColor = WorkingColor::new([1.0, 0.0, 0.0, 1.0]);
    const BLUE: WorkingColor = WorkingColor::new([0.0, 0.0, 1.0, 1.0]);
    const PAINTED_ICON: &str = include_str!("../tests/data/painted_icon.svg");

    /// The paints of `svg`, drawn once with the tint `tint`.
    fn record(svg: &Svg, tint: &Computed<WorkingColor>) -> Recording {
        let mut recording = Recording::default();
        let document = svg.document().expect("the fixture is a drawable SVG");
        render(&mut recording, &document, tint);
        recording
    }

    /// The tint is a live paint: moving it changes what the recorded commands
    /// paint with, and the document is not drawn again — no command is added,
    /// and nothing walks the tree.
    #[test]
    fn a_tint_change_repaints_the_commands_without_recording_again() {
        let tint = binding(RED);
        let svg = Svg::from_path("M3 3h18v18H3z", 24.0, 24.0);
        let recording = record(&svg, &tint.computed());
        assert_eq!(recording.commands, 1, "one path, one fill");
        let [paint] = &recording.paints[..] else {
            panic!("one fill has one paint");
        };
        assert_eq!(paint.snapshot(), Paint::Color(RED));

        tint.set(BLUE);
        assert_eq!(paint.snapshot(), Paint::Color(BLUE));
        assert_eq!(recording.commands, 1, "the change recorded nothing");
    }

    /// A stroke icon is tinted through its stroke, and the tint's own
    /// translucency carries through.
    #[test]
    fn a_stroke_takes_the_tint_with_its_alpha() {
        let tint = binding(WorkingColor::new([1.0, 0.0, 0.0, 0.5]));
        let svg = Svg::from_stroke_path("M3 12h18", 24.0, 24.0);
        let recording = record(&svg, &tint.computed());
        let [paint] = &recording.paints[..] else {
            panic!("one stroke has one paint");
        };
        assert_eq!(
            paint.snapshot(),
            Paint::Color(WorkingColor::new([1.0, 0.0, 0.0, 0.5]))
        );
    }

    /// `currentColor` in a gradient stop is the tint too, and the gradient
    /// keeps the stops that are not.
    #[test]
    fn a_gradient_stop_can_be_the_tint() {
        let svg = Svg::new(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 8 8">
                <linearGradient id="g"><stop offset="0" stop-color="currentColor"/><stop offset="1" stop-color="#ffffff"/></linearGradient>
                <rect width="8" height="8" fill="url(#g)"/>
            </svg>"##,
        )
        .viewbox(8.0, 8.0);
        let tint = binding(RED);
        let recording = record(&svg, &tint.computed());
        let [paint] = &recording.paints[..] else {
            panic!("one fill has one paint");
        };
        // A gradient in the shape's bounding box is placed by a transform.
        let stops = |paint: &Paint| match paint {
            Paint::Transformed(placed) => match &*placed.paint {
                Paint::Linear(gradient) => gradient
                    .stops
                    .iter()
                    .map(|stop| stop.color)
                    .collect::<Vec<_>>(),
                other => panic!("expected a linear gradient, got {other:?}"),
            },
            Paint::Linear(gradient) => gradient
                .stops
                .iter()
                .map(|stop| stop.color)
                .collect::<Vec<_>>(),
            other => panic!("expected a linear gradient, got {other:?}"),
        };
        let first = stops(&paint.snapshot());
        assert_eq!(first[0], RED);
        tint.set(BLUE);
        let second = stops(&paint.snapshot());
        assert_eq!(second[0], BLUE);
        assert_eq!(second[1], first[1], "the white stop is the document's own");
    }

    /// A document whose colours are its own does not follow the tint.
    #[test]
    fn a_document_in_its_own_colours_ignores_the_tint() {
        let tint = binding(RED);
        let svg = Svg::new(PAINTED_ICON);
        let recording = record(&svg, &tint.computed());
        let before: Vec<Paint> = recording.paints.iter().map(Computed::snapshot).collect();
        tint.set(BLUE);
        let after: Vec<Paint> = recording.paints.iter().map(Computed::snapshot).collect();
        assert_ne!(before, [] as [waterui_graphics::canvas::Paint; 0]);
        assert_eq!(before, after);
    }

    /// A colour the document spells itself is not the tint, whichever way it
    /// spells the marker colour: as `rgb(…)`, as a name, in a style
    /// attribute or in a stylesheet.
    #[test]
    fn a_document_spelling_a_marker_colour_itself_is_not_tinted() {
        for (spelling, svg) in [
            (
                "rgb",
                r#"<rect width="8" height="8" fill="rgb(255, 0, 0)"/>"#,
            ),
            ("name", r#"<rect width="8" height="8" fill="red"/>"#),
            ("name", r#"<rect width="8" height="8" fill="blue"/>"#),
            ("rgb", r#"<rect width="8" height="8" fill="rgb(0 0 255)"/>"#),
            (
                "style",
                r#"<rect width="8" height="8" style="fill: rgb(255,0,0)"/>"#,
            ),
            (
                "stylesheet",
                r#"<style>rect { fill: blue }</style><rect width="8" height="8"/>"#,
            ),
            (
                "stroke",
                r#"<rect width="8" height="8" fill="none" stroke="red"/>"#,
            ),
        ] {
            // The second shape asks for the tint, so the two parses of the
            // document really do differ, and the first is the one under test.
            let svg = Svg::new(alloc::format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 8 8">{svg}<circle cx="4" cy="4" r="2" fill="currentColor"/></svg>"#
            ))
            .viewbox(8.0, 8.0);
            let tint = binding(WorkingColor::new([0.0, 1.0, 0.0, 1.0]));
            let recording = record(&svg, &tint.clone().computed());
            let [own, asked] = &recording.paints[..] else {
                panic!("{spelling}: two shapes have two paints");
            };
            let (own_before, asked_before) = (own.snapshot(), asked.snapshot());
            tint.set(WorkingColor::new([1.0, 1.0, 0.0, 1.0]));
            assert_eq!(
                own.snapshot(),
                own_before,
                "{spelling}: the document's own colour must not follow the tint"
            );
            assert_ne!(
                asked.snapshot(),
                asked_before,
                "{spelling}: currentColor follows the tint"
            );
        }
    }

    /// The same document asking for the tint, beside colours that spell the
    /// markers, follows the tint only where it asks.
    #[test]
    fn only_current_color_follows_the_tint_beside_marker_spellings() {
        let svg = Svg::new(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 8 8">
                <rect width="4" height="8" fill="red"/>
                <rect x="4" width="4" height="8" fill="currentColor"/>
            </svg>"#,
        )
        .viewbox(8.0, 8.0);
        let tint = binding(RED);
        let recording = record(&svg, &tint.computed());
        let [own, tinted] = &recording.paints[..] else {
            panic!("two shapes have two paints");
        };
        let green = WorkingColor::new([0.0, 1.0, 0.0, 1.0]);
        tint.set(green);
        assert_eq!(tinted.snapshot(), Paint::Color(green));
        assert_ne!(own.snapshot(), Paint::Color(green));
    }

    /// Full markup takes its size from its own root element, and its
    /// `currentColor` follows the tint all the same.
    #[test]
    fn full_markup_without_a_viewbox_override_follows_the_tint() {
        let svg = Svg::new(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8" viewBox="0 0 8 8">
                <rect width="8" height="8" fill="currentColor"/>
            </svg>"#,
        );
        let tint = binding(RED);
        let recording = record(&svg, &tint.computed());
        let [tinted] = &recording.paints[..] else {
            panic!("one shape has one paint");
        };
        assert_eq!(tinted.snapshot(), Paint::Color(RED));
    }
}
