//! Draws a parsed SVG document through the engine's [`Draw`] recorder.
//!
//! `vello_svg` renders into a `vello::Scene` and nothing else, which ties every
//! icon in an application to one rendering engine. Recording into a
//! [`cherenkov::Draw`] instead lets the same document render on whichever
//! Cherenkov backend the host runs — GPU or the CPU rasteriser alike — and the
//! recording is a [`cherenkov::Content`] any engine layer can mount.
//!
//! The translation follows `vello_svg`'s own renderer, which is where the
//! handling of paint order, clip paths, nested documents and flattened text
//! comes from.

use alloc::vec::Vec;

use cherenkov::kurbo::{Affine, BezPath, Point, Rect, Shape as _, Stroke};
use cherenkov::{
    BlendMode, Color, Draw, EvenOdd, Extend, Fixed, Group, Interpolation, LinearGradient, Paint,
    RadialGradient, Srgb, WorkingColor,
};

use crate::usvg;

/// A [`Draw`] recorder whose operands accept recorded constants — every
/// operand this renderer produces.
///
/// [`StaticRecorder`][cherenkov::StaticRecorder] satisfies it because its
/// `Value` is [`Fixed`] itself, and [`Recorder`][cherenkov::Recorder] because
/// `Fixed` converts into its `Live` signal. Sealed to the two of them: the
/// bounds name each `Fixed` operand type, which is what a recorder must
/// accept for this renderer to write into it.
pub trait RecordSvg:
    Draw<
        Value<BezPath>: From<Fixed<BezPath>>,
        Value<EvenOdd<BezPath>>: From<Fixed<EvenOdd<BezPath>>>,
        Value<Paint>: From<Fixed<Paint>>,
        Value<Stroke>: From<Fixed<Stroke>>,
        Value<Group>: From<Fixed<Group>>,
        Value<Affine>: From<Fixed<Affine>>,
    >
{
}

impl<T> RecordSvg for T where
    T: Draw<
            Value<BezPath>: From<Fixed<BezPath>>,
            Value<EvenOdd<BezPath>>: From<Fixed<EvenOdd<BezPath>>>,
            Value<Paint>: From<Fixed<Paint>>,
            Value<Stroke>: From<Fixed<Stroke>>,
            Value<Group>: From<Fixed<Group>>,
            Value<Affine>: From<Fixed<Affine>>,
        >
{
}

/// Draws a whole document into `scene`, positioned by `base`.
pub fn render_tree<D: RecordSvg>(scene: &mut D, tree: &usvg::Tree, base: Affine) {
    render_group(scene, tree.root(), base);
}

/// Draws a group's children.
///
/// `base` places the document; every node carries its own *absolute* transform,
/// so the two combine per node and `base` is passed down unchanged. Handing
/// children an identity base instead loses the document's placement for
/// everything inside a group.
fn render_group<D: RecordSvg>(scene: &mut D, group: &usvg::Group, base: Affine) {
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

fn render_nested_group<D: RecordSvg>(
    scene: &mut D,
    group: &usvg::Group,
    base: Affine,
    transform: Affine,
) {
    let alpha = group.opacity().get();
    let blend = to_blend_mode(group.blend_mode());

    // A clip path with a single path clips to it; anything else clips to the
    // group's bounding box, which is what `vello_svg` does. The layer
    // transform vello applied to the clip element is the node's absolute
    // placement, so the clip shape is carried into that space here. A group
    // without a clip path draws its children unclipped — the bounding-box
    // fallback never cropped anything.
    let clip = group.clip_path().map(|clip_path| {
        let shape = clip_path
            .root()
            .children()
            .first()
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
        transform * shape
    });

    let isolated = group.opacity() != usvg::NormalizedF32::ONE || blend != BlendMode::Normal;
    match (clip, isolated) {
        (Some(clip), true) => {
            scene.group(Fixed(Group::new().opacity(alpha).blend(blend)), |scene| {
                scene.clip(Fixed(clip), |scene| render_group(scene, group, base));
            });
        }
        (Some(clip), false) => {
            scene.clip(Fixed(clip), |scene| render_group(scene, group, base));
        }
        (None, true) => {
            scene.group(Fixed(Group::new().opacity(alpha).blend(blend)), |scene| {
                render_group(scene, group, base);
            });
        }
        (None, false) => render_group(scene, group, base),
    }
}

fn render_path<D: RecordSvg>(scene: &mut D, path: &usvg::Path, transform: Affine) {
    if !path.is_visible() {
        return;
    }
    let outline = to_bez_path(path);

    let draw_fill = |scene: &mut D| {
        if let Some(fill) = path.fill()
            && let Some((paint, paint_transform)) = to_paint(fill.paint(), fill.opacity())
        {
            let paint = match paint_transform {
                Some(transform) => paint.transformed(transform),
                None => paint,
            };
            scene.transform(Fixed(transform), |scene| match fill.rule() {
                usvg::FillRule::NonZero => scene.fill(Fixed(outline.clone()), Fixed(paint.clone())),
                usvg::FillRule::EvenOdd => {
                    scene.fill(Fixed(EvenOdd(outline.clone())), Fixed(paint.clone()));
                }
            });
        }
    };
    let draw_stroke = |scene: &mut D| {
        if let Some(stroke) = path.stroke()
            && let Some((paint, paint_transform)) = to_paint(stroke.paint(), stroke.opacity())
        {
            let paint = match paint_transform {
                Some(transform) => paint.transformed(transform),
                None => paint,
            };
            scene.transform(Fixed(transform), |scene| {
                scene.stroke(
                    Fixed(outline.clone()),
                    Fixed(to_stroke(stroke)),
                    Fixed(paint),
                );
            });
        }
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

fn render_image<D: RecordSvg>(scene: &mut D, image: &usvg::Image, transform: Affine) {
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

/// A colour from SVG's sRGB8 components, in the engine's working space.
fn to_working_color(red: u8, green: u8, blue: u8, alpha: u8) -> WorkingColor {
    Color::<Srgb>::new([
        f32::from(red) / 255.0,
        f32::from(green) / 255.0,
        f32::from(blue) / 255.0,
        f32::from(alpha) / 255.0,
    ])
    .into()
}

/// The paint a fill or stroke uses, alongside the gradient's own
/// `gradientTransform` when it has one.
///
/// The paint transform maps paint coordinates into the shape's space — it is
/// deliberately not the recording transform, which places the geometry; SVG
/// keeps the two transforms separate and so does this port.
///
/// A pattern paints with a whole subtree rather than a paint, which no `Draw`
/// verb expresses; such a shape is left undrawn rather than painted a wrong
/// colour.
fn to_paint(paint: &usvg::Paint, opacity: usvg::Opacity) -> Option<(Paint, Option<Affine>)> {
    match paint {
        usvg::Paint::Color(color) => Some((
            Paint::Solid(to_working_color(
                color.red,
                color.green,
                color.blue,
                opacity.to_u8(),
            )),
            None,
        )),
        usvg::Paint::LinearGradient(gradient) => Some((
            Paint::Linear(LinearGradient {
                start: Point::new(f64::from(gradient.x1()), f64::from(gradient.y1())),
                end: Point::new(f64::from(gradient.x2()), f64::from(gradient.y2())),
                stops: to_stops(gradient.stops(), opacity),
                extend: to_extend(gradient.spread_method()),
                // SVG gradients interpolate in sRGB.
                interpolation: Interpolation::SrgbEncoded,
            }),
            to_paint_transform(&gradient.transform()),
        )),
        usvg::Paint::RadialGradient(gradient) => Some((
            Paint::Radial(RadialGradient {
                // The focal point has no radius of its own in this SVG model.
                start_center: Point::new(f64::from(gradient.fx()), f64::from(gradient.fy())),
                start_radius: 0.0,
                end_center: Point::new(f64::from(gradient.cx()), f64::from(gradient.cy())),
                end_radius: f64::from(gradient.r().get()),
                stops: to_stops(gradient.stops(), opacity),
                extend: to_extend(gradient.spread_method()),
                interpolation: Interpolation::SrgbEncoded,
            }),
            to_paint_transform(&gradient.transform()),
        )),
        usvg::Paint::Pattern(_) => None,
    }
}

fn to_stops(stops: &[usvg::Stop], opacity: usvg::Opacity) -> Vec<cherenkov::ColorStop> {
    stops
        .iter()
        .map(|stop| cherenkov::ColorStop {
            offset: stop.offset().get(),
            color: to_working_color(
                stop.color().red,
                stop.color().green,
                stop.color().blue,
                (stop.opacity() * opacity).to_u8(),
            ),
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

/// The gradient's own `gradientTransform`, when it has one.
///
/// `None` leaves the paint in the shape's space, which is what a gradient
/// without a `gradientTransform` wants.
fn to_paint_transform(transform: &usvg::Transform) -> Option<Affine> {
    (!transform.is_identity()).then(|| to_affine(transform))
}
