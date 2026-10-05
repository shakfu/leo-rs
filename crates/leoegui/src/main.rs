//! leoegui: a desktop front end for leolib, drawn with egui.
//!
//!     leoegui FILE.leo                         edit an outline
//!     leoegui F.leo --press "l,l" --screenshot out.ppm
//!                                              press keys, save one frame, exit
//!
//! Everything but drawing is `leoapp`, as in leotui: the same bindings,
//! commands and vim body. The window around them is an editor's: menus, a
//! command palette, an outline sidebar, tabs, a status bar and a panel.

mod editor;
mod gui;
mod input;
mod menus;
mod palette;
mod panel;
mod prompts;
mod settings;
mod status;
mod style;
mod tabs;
mod themes;
mod tree;

use std::path::PathBuf;

use clap::Parser;
use eframe::egui;
use leoapp::app;
use leoapp::theme::Depth;

/// A desktop front end for leolib.
#[derive(Parser)]
#[command(name = "leoegui", version)]
struct Args {
    /// The outline to open. Without one, leoegui starts an unsaved outline.
    #[arg(value_name = "FILE.leo")]
    path: Option<String>,
    /// Open the outline without reading its external files.
    #[arg(long)]
    no_external: bool,
    /// Use this theme for one launch, without saving it.
    #[arg(long, value_name = "NAME")]
    theme: Option<String>,
    /// Press keys after opening: binding specs separated by commas.
    #[arg(long, value_name = "KEYS", action = clap::ArgAction::Append)]
    press: Vec<String>,
    /// Save a frame as a binary PPM and exit: the first drawn two seconds after
    /// opening, so a language server has had time to answer.
    #[arg(long, value_name = "PATH")]
    screenshot: Option<PathBuf>,
}

fn main() -> eframe::Result {
    let args = Args::parse();
    let (mut app, settings) = match app::launch(
        args.path.as_deref(),
        !args.no_external,
        args.theme.as_deref(),
    ) {
        Ok(opened) => opened,
        Err(e) => {
            eprintln!("leoegui: {e}");
            std::process::exit(1);
        }
    };
    // A window has every colour; `NO_COLOR` still asks for none.
    if app.depth != Depth::None {
        app.depth = Depth::True;
    }
    let specs = args.press.iter().flat_map(|a| a.split(',')).map(str::trim);
    for spec in specs {
        for key in leoapp::keys::parse(spec) {
            app.handle_key(leoapp::keys::KeyEvent::new(key.code, key.mods));
        }
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([640.0, 400.0])
            .with_title(gui::title(&app)),
        event_loop_builder: Some(Box::new(|_builder| {
            // winit's own Quit item ends the process at once, unsaved work
            // and all. Without its menu, Cmd-Q reaches the app, which asks.
            #[cfg(target_os = "macos")]
            {
                use winit::platform::macos::EventLoopBuilderExtMacOS;
                _builder.with_default_menu(false);
            }
        })),
        ..Default::default()
    };
    let cmd_is_ctrl = !settings.mac_dont_swap_ctrl_and_meta;
    eframe::run_native(
        "leoegui",
        options,
        Box::new(move |cc| {
            // A server's message repaints, so it is drawn without a key.
            let ctx = cc.egui_ctx.clone();
            let wake: std::sync::Arc<dyn Fn() + Send + Sync> =
                std::sync::Arc::new(move || ctx.request_repaint());
            if let Some(lsp) = app.lsp.as_mut() {
                lsp.set_wake(wake.clone());
            }
            if let Some(mcp) = &app.mcp {
                mcp.set_wake(wake);
            }
            let gui = gui::Gui::new(
                &cc.egui_ctx,
                app,
                cmd_is_ctrl,
                args.screenshot,
                &settings,
                args.theme.as_deref(),
            );
            Ok(Box::new(gui))
        }),
    )
}

/// Save `image` as a binary PPM, which needs no encoder.
pub fn write_ppm(path: &std::path::Path, image: &egui::ColorImage) -> std::io::Result<()> {
    let [w, h] = image.size;
    let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
    for p in &image.pixels {
        out.extend_from_slice(&[p.r(), p.g(), p.b()]);
    }
    std::fs::write(path, out)
}
