//! Semantics checks for the `svg` component at its public surface.
//!
//! The mounted-runtime assertions this file used to carry — an `a11y_label`
//! winning over the document's `<title>`, and an explicit `.size(…)` winning
//! over the drawing's intrinsic size — mount through `waterui-testing`, which
//! needs a Hydrolysis backend. No Hydrolysis revision compiles against the
//! engine API this crate records into yet, so those checks are unreachable
//! here and belong to the test host once it is ported. What remains is the
//! part this crate owns: intrinsic size, and that a document actually records
//! drawing commands.

use cherenkov::Recorder;
use waterui_core::layout::Size;
use waterui_graphics::SceneContent;
use waterui_svg::SvgSceneContent;

const TITLED_ICON: &str = include_str!("data/titled_icon.svg");
const STROKED_ICON: &str = include_str!("data/stroked_icon.svg");
const PAINTED_ICON: &str = include_str!("data/painted_icon.svg");

#[test]
fn scene_content_reports_the_documents_intrinsic_size() {
    assert_eq!(
        SvgSceneContent::new(TITLED_ICON).intrinsic_size(),
        Some(Size::new(24.0, 24.0))
    );
    assert_eq!(
        SvgSceneContent::new(PAINTED_ICON).intrinsic_size(),
        Some(Size::new(48.0, 48.0))
    );
}

#[test]
fn scene_content_records_drawing_commands() {
    let mut recorder = Recorder::new();
    SvgSceneContent::new(STROKED_ICON).build_scene(&mut recorder, 48.0, 48.0);
    let content = recorder.finish();
    assert!(!content.is_empty(), "a stroked icon must record commands");
}
