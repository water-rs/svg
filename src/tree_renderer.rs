//! Draws a parsed SVG document as Cherenkov content.
//!
//! The document is walked node by node into whichever [`Draw`] is recording —
//! a static picture or a live scene — so an icon renders on every Cherenkov
//! backend and inside any backend that merges the content into a scene of its
//! own.
//!
//! The translation follows `vello_svg`'s renderer, which is where the handling
//! of paint order, clip paths, nested documents and flattened text comes from.

use alloc::vec::Vec;

use cherenkov::kurbo::{Affine, BezPath, Rect, Shape as _, Stroke};
use cherenkov::{
    BlendMode, ColorStop, Draw, EvenOdd, Extend, Group, Interpolation, LinearGradient, Paint,
    RadialGradient, WorkingColor,
};
use color::{AlphaColor, Srgb};

use crate::usvg;

/// A [`Draw`] that accepts plain values, which is every recorder Cherenkov has:
/// a static picture takes [`Fixed`](cherenkov::Fixed) values and a live scene
/// takes [`Live`](cherenkov::Live) ones, and both are built from the value.
pub trait SvgTarget:
    Draw<
        Value<BezPath>: From<BezPath>,
        Value<EvenOdd<BezPath>>: From<EvenOdd<BezPath>>,
        Value<Paint>: From<Paint>,
        Value<Stroke>: From<Stroke>,
        Value<Affine>: From<Affine>,
        Value<Group>: From<Group>,
    >
{
}

impl<D> SvgTarget for D where
    D: Draw<
            Value<BezPath>: From<BezPath>,
            Value<EvenOdd<BezPath>>: From<EvenOdd<BezPath>>,
            Value<Paint>: From<Paint>,
            Value<Stroke>: From<Stroke>,
            Value<Affine>: From<Affine>,
            Value<Group>: From<Group>,
        >
{
}

/// Draws a whole document into `scene`, positioned by `base`.
pub fn render_tree<D: SvgTarget>(scene: &mut D, tree: &usvg::Tree, base: Affine) {
    render_group(scene, tree.root(), base);
}

/// Draws a group's children.
///
/// `base` places the document; every node carries its own *absolute* transform,
/// so the two combine per node and `base` is passed down unchanged. Handing
/// children an identity base instead loses the document's placement for
/// everything inside a group.
fn render_group<D: SvgTarget>(scene: &mut D, group: &usvg::Group, base: Affine) {
    for node in group.children() {
        let transform = base * to_affine(&node.abs_transform());
        match node {
            usvg::Node::Group(group) => render_nested_group(scene, group, base, transform),
            usvg::Node::Path(path) => render_path(scene, path, transform),
            usvg::Node::Image(image) => render_image(scene, image, transform),
            // Text arrives already flattened into outlines, so this draws
            // glyphs the same way it draws any other geometry. The parser only
            // produces such a node when it had both the `text` feature and a
            // font to lay the text out with; otherwise it drops the `<text>`
            // element and this arm never runs.
            usvg::Node::Text(text) => render_group(scene, text.flattened(), base),
        }
    }
}

fn render_nested_group<D: SvgTarget>(
    scene: &mut D,
    group: &usvg::Group,
    base: Affine,
    transform: Affine,
) {
    let alpha = group.opacity().get();
    let blend = to_blend_mode(group.blend_mode());

    // A clip path with a single path clips to it; anything else clips to the
    // group's bounding box, which is what `vello_svg` does and keeps the
    // nesting balanced either way.
    let clip = group
        .clip_path()
        .and_then(|path| path.root().children().first())
        .and_then(|node| match node {
            usvg::Node::Path(path) => Some(to_bez_path(path)),
            _ => None,
        })
        .unwrap_or_else(|| {
            let bounds = group.layer_bounding_box();
            let rect = Rect::from_origin_size(
                (f64::from(bounds.x()), f64::from(bounds.y())),
                (f64::from(bounds.width()), f64::from(bounds.height())),
            );
            let mut path = BezPath::new();
            path.extend(rect.path_elements(0.1));
            path
        });
    let clip = transform * clip;

    let isolated = alpha < 1.0 || blend != BlendMode::Normal;
    let body = |scene: &mut D| scene.clip(clip, |scene| render_group(scene, group, base));
    if isolated {
        scene.group(Group::new().opacity(alpha).blend(blend), body);
    } else {
        body(scene);
    }
}

fn render_path<D: SvgTarget>(scene: &mut D, path: &usvg::Path, transform: Affine) {
    if !path.is_visible() {
        return;
    }
    let outline = to_bez_path(path);

    let draw_fill = |scene: &mut D| {
        let Some(fill) = path.fill() else {
            return;
        };
        let Some((paint, paint_transform)) = to_paint(fill.paint(), fill.opacity()) else {
            return;
        };
        with_paint_transform(
            scene,
            transform,
            paint_transform,
            |scene, shape_transform| {
                let outline = shape_transform * outline.clone();
                match fill.rule() {
                    usvg::FillRule::NonZero => scene.fill(outline, paint),
                    usvg::FillRule::EvenOdd => scene.fill(EvenOdd(outline), paint),
                }
            },
        );
    };
    let draw_stroke = |scene: &mut D| {
        let Some(stroke) = path.stroke() else {
            return;
        };
        let Some((paint, paint_transform)) = to_paint(stroke.paint(), stroke.opacity()) else {
            return;
        };
        let stroke = to_stroke(stroke);
        // A stroke's width lives in the path's own units, so the stroke is
        // recorded under the node's transform rather than pre-transformed.
        scene.transform(transform, |scene| match paint_transform {
            None => scene.stroke(outline.clone(), stroke, paint),
            Some(paint_transform) => {
                let inverse = paint_transform.inverse();
                scene.transform(paint_transform, |scene| {
                    scene.stroke(inverse * outline.clone(), stroke, paint);
                });
            }
        });
    };

    match path.paint_order() {
        usvg::PaintOrder::FillAndStroke => {
            draw_fill(scene);
            draw_stroke(scene);
        }
        usvg::PaintOrder::StrokeAndFill => {
            draw_stroke(scene);
            draw_fill(scene);
        }
    }
}

/// Runs `body` so that a paint with its own transform lands where SVG puts it.
///
/// Cherenkov paints are expressed in the shape's coordinates, so a
/// `gradientTransform` becomes a recording transform with the shape mapped
/// through its inverse: the shape stays where it was and the gradient moves.
fn with_paint_transform<D: SvgTarget>(
    scene: &mut D,
    transform: Affine,
    paint_transform: Option<Affine>,
    body: impl FnOnce(&mut D, Affine),
) {
    match paint_transform {
        None => body(scene, transform),
        Some(paint_transform) => {
            let inverse = paint_transform.inverse();
            scene.transform(transform * paint_transform, |scene| body(scene, inverse));
        }
    }
}

fn render_image<D: SvgTarget>(scene: &mut D, image: &usvg::Image, transform: Affine) {
    if !image.is_visible() {
        return;
    }
    match image.kind() {
        // A nested document is placed by this node, and its own nodes carry
        // absolute transforms within it, so it becomes the base in there.
        usvg::ImageKind::SVG(svg) => render_group(scene, svg.root(), transform),
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

fn to_affine(transform: &usvg::Transform) -> Affine {
    Affine::new([
        f64::from(transform.sx),
        f64::from(transform.ky),
        f64::from(transform.kx),
        f64::from(transform.sy),
        f64::from(transform.tx),
        f64::from(transform.ty),
    ])
}

fn to_bez_path(path: &usvg::Path) -> BezPath {
    let mut local = BezPath::new();
    for segment in path.data().segments() {
        match segment {
            usvg::tiny_skia_path::PathSegment::MoveTo(point) => {
                local.move_to((f64::from(point.x), f64::from(point.y)));
            }
            usvg::tiny_skia_path::PathSegment::LineTo(point) => {
                local.line_to((f64::from(point.x), f64::from(point.y)));
            }
            usvg::tiny_skia_path::PathSegment::QuadTo(control, end) => {
                local.quad_to(
                    (f64::from(control.x), f64::from(control.y)),
                    (f64::from(end.x), f64::from(end.y)),
                );
            }
            usvg::tiny_skia_path::PathSegment::CubicTo(first, second, end) => {
                local.curve_to(
                    (f64::from(first.x), f64::from(first.y)),
                    (f64::from(second.x), f64::from(second.y)),
                    (f64::from(end.x), f64::from(end.y)),
                );
            }
            usvg::tiny_skia_path::PathSegment::Close => local.close_path(),
        }
    }
    local
}

fn to_stroke(stroke: &usvg::Stroke) -> Stroke {
    Stroke::new(f64::from(stroke.width().get()))
        .with_caps(match stroke.linecap() {
            usvg::LineCap::Butt => cherenkov::kurbo::Cap::Butt,
            usvg::LineCap::Round => cherenkov::kurbo::Cap::Round,
            usvg::LineCap::Square => cherenkov::kurbo::Cap::Square,
        })
        .with_join(match stroke.linejoin() {
            usvg::LineJoin::Miter | usvg::LineJoin::MiterClip => cherenkov::kurbo::Join::Miter,
            usvg::LineJoin::Round => cherenkov::kurbo::Join::Round,
            usvg::LineJoin::Bevel => cherenkov::kurbo::Join::Bevel,
        })
        .with_miter_limit(f64::from(stroke.miterlimit().get()))
        .with_dashes(
            f64::from(stroke.dashoffset()),
            stroke
                .dasharray()
                .map(|array| array.iter().copied().map(f64::from))
                .into_iter()
                .flatten(),
        )
}

fn to_color(color: usvg::Color, opacity: usvg::Opacity) -> WorkingColor {
    cherenkov::Color::<Srgb>::from(AlphaColor::<Srgb>::from_rgba8(
        color.red,
        color.green,
        color.blue,
        opacity.to_u8(),
    ))
    .to_working()
}

/// The paint a fill or stroke uses, with the transform that positions it.
///
/// A pattern paints with a whole subtree rather than a paint, which no
/// Cherenkov command expresses; such a shape is left undrawn rather than
/// painted a wrong colour.
fn to_paint(paint: &usvg::Paint, opacity: usvg::Opacity) -> Option<(Paint, Option<Affine>)> {
    match paint {
        usvg::Paint::Color(color) => Some((Paint::Solid(to_color(*color, opacity)), None)),
        usvg::Paint::LinearGradient(gradient) => Some((
            Paint::Linear(LinearGradient {
                start: (f64::from(gradient.x1()), f64::from(gradient.y1())).into(),
                end: (f64::from(gradient.x2()), f64::from(gradient.y2())).into(),
                stops: to_stops(gradient.stops(), opacity),
                extend: to_extend(gradient.spread_method()),
                interpolation: Interpolation::default(),
            }),
            to_paint_transform(&gradient.transform()),
        )),
        usvg::Paint::RadialGradient(gradient) => Some((
            Paint::Radial(RadialGradient {
                // The focal point has no radius of its own in this SVG model.
                start_center: (f64::from(gradient.fx()), f64::from(gradient.fy())).into(),
                start_radius: 0.0,
                end_center: (f64::from(gradient.cx()), f64::from(gradient.cy())).into(),
                end_radius: f64::from(gradient.r().get()),
                stops: to_stops(gradient.stops(), opacity),
                extend: to_extend(gradient.spread_method()),
                interpolation: Interpolation::default(),
            }),
            to_paint_transform(&gradient.transform()),
        )),
        usvg::Paint::Pattern(_) => None,
    }
}

fn to_stops(stops: &[usvg::Stop], opacity: usvg::Opacity) -> Vec<ColorStop> {
    stops
        .iter()
        .map(|stop| ColorStop {
            offset: stop.offset().get(),
            color: to_color(stop.color(), stop.opacity() * opacity),
        })
        .collect()
}

const fn to_extend(spread: usvg::SpreadMethod) -> Extend {
    match spread {
        usvg::SpreadMethod::Pad => Extend::Pad,
        usvg::SpreadMethod::Reflect => Extend::Reflect,
        usvg::SpreadMethod::Repeat => Extend::Repeat,
    }
}

/// The gradient's own transform, when it has one.
///
/// `None` leaves the paint in the shape's coordinates, which is what a
/// gradient without a `gradientTransform` wants.
fn to_paint_transform(transform: &usvg::Transform) -> Option<Affine> {
    (!transform.is_identity()).then(|| to_affine(transform))
}
