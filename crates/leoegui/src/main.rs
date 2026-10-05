//! leoegui, drawn with wgpu: Vulkan, Metal or DirectX 12 as the platform
//! has. See the library for what it does.

fn main() -> eframe::Result {
    leoegui::run("leoegui", eframe::Renderer::Wgpu)
}
