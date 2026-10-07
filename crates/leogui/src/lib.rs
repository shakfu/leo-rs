//! leogui: a desktop front end for leolib, drawn with egui.
//!
//! ```text
//! leogui FILE.leo...                      edit outlines, a tab each
//! leogui                                  the outlines open at the last quit
//! leogui F.leo --press "l,l" --screenshot out.ppm
//!                                          press keys, save one frame, exit
//! ```
//!
//! Everything but drawing is `leoapp`, as in leotui: the same bindings,
//! commands and vim body. The window around them is an editor's: menus, a
//! command palette, an outline sidebar, tabs, a status bar and a panel.
//!
//! Two executables run it: `leogui`, drawn with wgpu, eframe's default, and
//! `leogui-glow`, drawn with OpenGL, to compare the two.

mod editor;
mod find;
mod goto;
mod gui;
mod input;
mod menus;
mod outlines;
mod palette;
mod panel;
mod prompts;
mod session;
mod settings;
mod status;
mod style;
mod tabs;
mod themes;
mod tree;

use std::path::PathBuf;

use clap::{CommandFactory, FromArgMatches, Parser};
use eframe::egui;
use leoapp::app;
use leoapp::theme::Depth;

/// A desktop front end for leolib.
#[derive(Parser)]
#[command(name = "leogui", version)]
struct Args {
    /// The outlines to open, a tab each. Without one, leogui opens those
    /// open when it last quit, or an unsaved outline.
    #[arg(value_name = "FILE.leo")]
    paths: Vec<String>,
    /// Neither restore the last session nor save this one.
    #[arg(long)]
    no_session: bool,
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

/// Parse the command line and run the window, drawn by `renderer`.
/// `name` is the program's, for messages and the window system.
pub fn run(name: &'static str, renderer: eframe::Renderer) -> eframe::Result {
    let matches = Args::command().name(name).get_matches();
    let args = Args::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());
    let (mut app, settings) = match app::launch(
        args.paths.first().map(String::as_str),
        !args.no_external,
        args.theme.as_deref(),
    ) {
        Ok(opened) => opened,
        Err(e) => {
            eprintln!("{name}: {e}");
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
        renderer,
        ..Default::default()
    };
    // Keys pressed from the command line are for the outline named, so a
    // session is restored only when nothing is.
    let saved = match args.paths.is_empty() && args.press.is_empty() && !args.no_session {
        true => session::path().and_then(|p| session::load(&p)),
        false => None,
    };
    let mut options = options;
    if let Some(size) = saved.as_ref().and_then(|s| s.window) {
        options.viewport = options.viewport.with_inner_size(size);
    }
    let cmd_is_ctrl = !settings.mac_dont_swap_ctrl_and_meta;
    eframe::run_native(
        name,
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
            let mut gui = gui::Gui::new(
                &cc.egui_ctx,
                app,
                cmd_is_ctrl,
                args.screenshot,
                &settings,
                args.theme.as_deref(),
            );
            for path in args.paths.iter().skip(1) {
                gui.open_outline(Some(path));
            }
            // `--press` went to the first.
            gui.switch_to(0);
            if let Some(saved) = &saved {
                gui.restore(saved);
            }
            if args.no_session {
                gui.session_path = None;
            }
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
