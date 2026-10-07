//! leogui drawn with glow, over OpenGL, to compare with the default wgpu:
//! faster to start and smaller where OpenGL works, deprecated on macOS.

// A window, not a console, on Windows; debug builds keep the console for
// `--help` and errors.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() -> eframe::Result {
    leogui::run("leogui-glow", eframe::Renderer::Glow)
}
