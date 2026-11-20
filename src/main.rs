#![windows_subsystem = "windows"]

mod ui;

use eframe::egui;
use lumodent::workspace::Workspace;
use lumodent::APP_NAME;
use std::path::PathBuf;

/// Where patient data lives. `LUMODENT_DATA` overrides it (portable installs, tests).
fn data_dir() -> Result<PathBuf, String> {
    if let Some(dir) = std::env::var_os("LUMODENT_DATA") {
        return Ok(PathBuf::from(dir));
    }
    let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
    #[cfg(windows)]
    {
        let root = var("LOCALAPPDATA").ok_or("LOCALAPPDATA is unavailable")?;
        let folder = root.join(APP_NAME);
        // Builds before the rename stored data here; move it once.
        let legacy = root.join("NanoPix-Rust");
        if legacy.is_dir() && !folder.exists() {
            std::fs::rename(&legacy, &folder).map_err(|e| e.to_string())?;
        }
        Ok(folder)
    }
    #[cfg(target_os = "macos")]
    {
        Ok(var("HOME").ok_or("HOME is unavailable")?.join("Library/Application Support").join(APP_NAME))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let base = var("XDG_DATA_HOME")
            .filter(|p| p.is_absolute())
            .or_else(|| var("HOME").map(|h| h.join(".local/share")))
            .ok_or("HOME is unavailable")?;
        Ok(base.join("lumodent"))
    }
}

/// Opens the data folder. When LumoDent is first installed on a PC that has
/// the original NanoPix, NanoPix's databases are used from then on (unless
/// `LUMODENT_DATA` is set), and its preferences and clinic details are copied
/// into LumoDent's files. A PC where LumoDent already has its own database
/// keeps using it.
fn open_workspace(data: PathBuf) -> Result<Workspace, String> {
    let marker = data.join("nanopix-data.txt");
    // A folder chosen in Settings (or found before) wins over the INI.
    let remembered = std::fs::read_to_string(&marker).ok().map(|s| PathBuf::from(s.trim()));
    if let Some(install) = remembered.as_deref().and_then(lumodent::nanopix::Install::from_folder) {
        return Workspace::open_nanopix(data, &install);
    }
    let own_db = data.join("patients.sqlite3").exists() && !marker.exists();
    let install = (std::env::var_os("LUMODENT_DATA").is_none() && !own_db)
        .then(lumodent::nanopix::ini_path)
        .flatten()
        .and_then(|ini| lumodent::nanopix::detect(&ini));
    let Some(install) = install else { return Workspace::open(data) };
    let ws = Workspace::open_nanopix(data, &install)?;
    std::fs::write(&marker, install.data.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
    let settings = ws.data.join("settings.txt");
    if !settings.exists() {
        lumodent::nanopix::import_settings(&install).save(&settings)?;
        if ws.load_clinic().name.is_empty() {
            ws.save_clinic(&lumodent::nanopix::import_clinic(&install))?;
        }
    }
    Ok(ws)
}

/// The window icon: the tooth mark on its dark disc (src/logo.rs).
fn window_icon() -> egui::IconData {
    use lumodent::logo;
    const N: usize = 64;
    egui::IconData { rgba: logo::render(N, true, logo::TOP, logo::BOTTOM), width: N as u32, height: N as u32 }
}

fn main() -> eframe::Result {
    let workspace = data_dir().and_then(open_workspace);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(APP_NAME)
            // Matches the Linux desktop entry (packaging/linux/lumodent.desktop).
            .with_app_id("lumodent")
            .with_inner_size([1200.0, 800.0])
            .with_min_inner_size([1024.0, 700.0])
            // LumoDent draws its own title bar with the menus (src/ui/mod.rs).
            .with_decorations(false)
            .with_resizable(true)
            .with_icon(window_icon()),
        ..Default::default()
    };
    eframe::run_native(
        APP_NAME,
        options,
        Box::new(move |cc| {
            let workspace = workspace.map_err(|e| format!("Cannot open the data folder: {e}"));
            let light = workspace.as_ref().is_ok_and(|w| ui::saved_light(&w.data));
            ui::theme::set_light(&cc.egui_ctx, light);
            ui::i18n::set_indonesian(workspace.as_ref().is_ok_and(|w| ui::saved_indonesian(&w.data)));
            let workspace = workspace?;
            Ok(Box::new(ui::App::new(workspace, ui::Hooks::native())))
        }),
    )
}
