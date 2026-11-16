//! leogui with every plugin, drawn with wgpu. It names itself leogui, for the
//! window system, as `make install` installs it under that name.

// A window, not a console, on Windows; debug builds keep the console.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() -> eframe::Result {
    leogui::run_with("leogui", eframe::Renderer::Wgpu, leo_all::register)
}
