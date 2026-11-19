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

/// The window icon: the same crescent-over-disc mark as the in-app logo.
fn window_icon() -> egui::IconData {
    const N: usize = 64;
    let mut rgba = vec![0u8; N * N * 4];
    let inside = |x: f32, y: f32, cx: f32, cy: f32, r: f32| (x - cx).powi(2) + (y - cy).powi(2) <= r * r;
    for y in 0..N {
        for x in 0..N {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let px = &mut rgba[(y * N + x) * 4..][..4];
            if inside(fx, fy, 32.0, 32.0, 30.0) {
                px.copy_from_slice(&[0x2c, 0x34, 0x3e, 0xff]);
                if inside(fx, fy, 32.0, 34.0, 20.0) && !inside(fx, fy, 38.0, 28.0, 17.0) {
                    px.copy_from_slice(&[0x8f, 0xb8, 0xe0, 0xff]);
                }
            }
        }
    }
    egui::IconData { rgba, width: N as u32, height: N as u32 }
}

fn main() -> eframe::Result {
    let workspace = data_dir().and_then(Workspace::open);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(APP_NAME)
            // Matches the Linux desktop entry (packaging/linux/lumodent.desktop).
            .with_app_id("lumodent")
            .with_inner_size([1200.0, 800.0])
            .with_min_inner_size([1024.0, 700.0])
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
            let workspace = workspace?;
            Ok(Box::new(ui::App::new(workspace, ui::Hooks::native())))
        }),
    )
}
