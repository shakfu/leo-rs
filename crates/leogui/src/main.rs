//! leogui, drawn with wgpu: Vulkan, Metal or DirectX 12 as the platform
//! has. See the library for what it does.

// A window, not a console, on Windows; debug builds keep the console for
// `--help` and errors.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() -> eframe::Result {
    leogui::run("leogui", eframe::Renderer::Wgpu)
}
