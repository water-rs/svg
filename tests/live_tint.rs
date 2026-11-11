//! The tint of a mounted `Svg` follows a signal: moving it repaints the drawing
//! in place. The images are written for review by eye.

use std::path::PathBuf;
use std::time::Duration;

use hydrolysis_m3::Material3;
use waterui_core::reactive::binding;
use waterui_graphics::color::{Color, signal_color};
use waterui_svg::Svg;
use waterui_testing::ui;

const TINTED_ICON: &str = include_str!("data/tinted_icon.svg");

fn output(name: &str) -> PathBuf {
    let directory =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/svg-artifacts/live_tint");
    std::fs::create_dir_all(&directory).expect("the output directory is writable");
    directory.join(name)
}

#[test]
fn a_tint_that_changes_repaints_the_mounted_drawing() {
    let tint = binding(Color::srgb(224, 64, 0));
    let mut app = ui()
        .viewport(192, 64)
        .theme(Material3::defaults())
        .mount_offscreen({
            let tint = tint.clone();
            move || {
                Svg::new(TINTED_ICON)
                    .viewbox(96.0, 32.0)
                    .tint(signal_color(tint.clone()))
            }
        });
    app.snapshot()
        .save_png(output("orange.png"))
        .expect("the snapshot is writable");

    tint.set(Color::srgb(0, 112, 128));
    app.pump_for(Duration::from_millis(16));
    app.snapshot()
        .save_png(output("teal.png"))
        .expect("the snapshot is writable");
}
