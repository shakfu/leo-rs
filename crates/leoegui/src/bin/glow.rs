//! leoegui drawn with glow, over OpenGL, to compare with the default wgpu:
//! faster to start and smaller where OpenGL works, deprecated on macOS.

fn main() -> eframe::Result {
    leoegui::run("leoegui-glow", eframe::Renderer::Glow)
}
