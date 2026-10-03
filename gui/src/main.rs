// No console window behind the app
#![windows_subsystem = "windows"]

use node_gui::i18n::{self, tr};
use node_gui::settings::Settings;
use node_gui::supervisor::{Phase, Status, Supervisor, STOP_TIMEOUT};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, State, WindowEvent, Wry};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_opener::OpenerExt;

struct AppState {
    sup: Arc<Supervisor>,
    settings_path: PathBuf,
}

#[tauri::command]
fn get_status(st: State<AppState>) -> Status {
    st.sup.status()
}

#[tauri::command]
fn get_log(st: State<AppState>, cursor: u64) -> Vec<(u64, String)> {
    st.sup.log_lines(cursor, 2000)
}

#[tauri::command]
fn node_start(st: State<AppState>) -> Result<(), String> {
    st.sup.start()
}

#[tauri::command]
fn node_stop(st: State<AppState>) -> Result<(), String> {
    st.sup.stop()
}

#[tauri::command]
fn node_restart(st: State<AppState>) -> Result<(), String> {
    st.sup.restart()
}

#[tauri::command]
fn node_kill(st: State<AppState>) -> Result<(), String> {
    st.sup.kill()
}

/// [code, name] pairs for the language picker
#[tauri::command]
fn get_languages() -> Vec<(String, String)> {
    i18n::languages()
}

#[tauri::command]
fn get_settings(st: State<AppState>) -> Settings {
    st.sup.settings()
}

#[tauri::command]
fn save_settings(st: State<AppState>, settings: Settings) -> Result<(), String> {
    settings.save(&st.settings_path)?;
    st.sup.set_settings(settings);
    Ok(())
}

/// Opens a file or folder in Explorer; a missing one gets a clear message instead of a shell error
fn open_existing(app: &AppHandle, path: &std::path::Path, lang: &str) -> Result<(), String> {
    if !path.exists() {
        return Err(i18n::trf(lang, "err.not_found", &[("path", path.display().to_string())]));
    }
    app.opener().open_path(path.display().to_string(), None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
fn open_data_dir(app: AppHandle, st: State<AppState>) -> Result<(), String> {
    let s = st.sup.settings();
    open_existing(&app, &s.data_dir, &s.language)
}

#[tauri::command]
fn open_log(app: AppHandle, st: State<AppState>) -> Result<(), String> {
    let s = st.sup.settings();
    open_existing(&app, &s.log_path(), &s.language)
}

#[tauri::command]
fn copy_rpc(app: AppHandle, st: State<AppState>) -> Result<(), String> {
    let ep = st.sup.settings().rpc_endpoint;
    app.clipboard().write_text(format!("ws://{ep}")).map_err(|e| e.to_string())
}

/// A filled circle in the status colour, drawn at runtime so there are no icon files to keep in sync.
fn status_icon(color: &str) -> Image<'static> {
    let (r, g, b) = match color {
        "green" => (0x2e, 0xa0, 0x43),
        "yellow" => (0xe0, 0xa1, 0x00),
        "red" => (0xd1, 0x24, 0x2f),
        _ => (0x8c, 0x95, 0x9f),
    };
    const N: u32 = 32;
    let mut px = Vec::with_capacity((N * N * 4) as usize);
    let c = (N as f32 - 1.0) / 2.0;
    for y in 0..N {
        for x in 0..N {
            let d = ((x as f32 - c).powi(2) + (y as f32 - c).powi(2)).sqrt();
            // 1 px of anti-aliasing at the edge
            let a = ((13.5 - d).clamp(0.0, 1.0) * 255.0) as u8;
            px.extend_from_slice(&[r, g, b, a]);
        }
    }
    Image::new_owned(px, N, N)
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        w.show().ok();
        w.unminimize().ok();
        w.set_focus().ok();
    }
}

/// "Выход": stops the node explicitly, never by ending the process tree.
fn quit(app: AppHandle) {
    std::thread::spawn(move || {
        let sup = app.state::<AppState>().sup.clone();
        if sup.is_node_running() {
            sup.stop().ok();
            if !sup.wait_stopped(STOP_TIMEOUT) {
                let lang = sup.settings().language;
                let kill = app
                    .dialog()
                    .message(tr(&lang, "dlg.quit_kill"))
                    .title("Graphene Node")
                    .kind(MessageDialogKind::Warning)
                    .buttons(MessageDialogButtons::OkCancelCustom(
                        tr(&lang, "dlg.kill_button"),
                        tr(&lang, "dlg.wait_button"),
                    ))
                    .blocking_show();
                if !kill {
                    return;
                }
                sup.kill().ok();
                sup.wait_stopped(Duration::from_secs(10));
            }
        }
        app.exit(0);
    });
}

/// "Stop and close" quits like the tray's Quit; "Keep running in background" (or dismissing the dialog,
/// which leaves the node alone) hides the window to the tray.
fn ask_on_close(app: AppHandle) {
    let lang = app.state::<AppState>().sup.settings().language;
    let a = app.clone();
    app.dialog()
        .message(tr(&lang, "dlg.close"))
        .title("Graphene Node")
        .kind(MessageDialogKind::Info)
        .buttons(MessageDialogButtons::OkCancelCustom(tr(&lang, "tray.quit"), tr(&lang, "dlg.close_background")))
        .show(move |stop| {
            if stop {
                if let Some(w) = a.get_webview_window("main") {
                    w.hide().ok();
                }
                quit(a);
            } else if let Some(w) = a.get_webview_window("main") {
                w.hide().ok();
            }
        });
}

struct TrayItems {
    status: MenuItem<Wry>,
    start: MenuItem<Wry>,
    stop: MenuItem<Wry>,
    restart: MenuItem<Wry>,
    /// Every item with a fixed label and its translation key, relabelled when the language changes
    labelled: Vec<(MenuItem<Wry>, String)>,
}

fn build_tray(app: &AppHandle, lang: &str) -> tauri::Result<(TrayIcon, TrayItems)> {
    // The menu id doubles as the translation key "tray.<id>"
    let item = |id: &str, enabled: bool| {
        MenuItem::with_id(app, id, tr(lang, &format!("tray.{id}")), enabled, None::<&str>)
    };
    let sep = || PredefinedMenuItem::separator(app);
    let status = MenuItem::with_id(app, "status", tr(lang, "status.stopped"), false, None::<&str>)?;
    let (open, start, stop, restart) = (item("open", true)?, item("start", true)?, item("stop", false)?, item("restart", false)?);
    let (copy, data, log, quit_item) =
        (item("copy_rpc", true)?, item("open_data", true)?, item("open_log", true)?, item("quit", true)?);
    let menu = Menu::with_items(
        app,
        &[
            &status, &sep()?, &open, &sep()?, &start, &stop, &restart, &sep()?, &copy, &data, &log, &sep()?, &quit_item,
        ],
    )?;
    let labelled = [&open, &start, &stop, &restart, &copy, &data, &log, &quit_item]
        .into_iter()
        .map(|m| (m.clone(), format!("tray.{}", m.id().as_ref())))
        .collect();
    let items = TrayItems { status, start, stop, restart, labelled };
    let tray = TrayIconBuilder::with_id("main")
        .icon(status_icon("gray"))
        .tooltip("Graphene Node")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, ev| {
            let st = app.state::<AppState>();
            let result = match ev.id().as_ref() {
                "open" => {
                    show_main(app);
                    Ok(())
                }
                "start" => st.sup.start(),
                "stop" => st.sup.stop(),
                "restart" => st.sup.restart(),
                "copy_rpc" => copy_rpc(app.clone(), st.clone()),
                "open_data" => open_data_dir(app.clone(), st.clone()),
                "open_log" => open_log(app.clone(), st.clone()),
                "quit" => {
                    quit(app.clone());
                    Ok(())
                }
                _ => Ok(()),
            };
            if let Err(e) = result {
                app.dialog().message(e).title("Graphene Node").kind(MessageDialogKind::Error).show(|_| {});
            }
        })
        .on_tray_icon_event(|tray, ev| {
            if let TrayIconEvent::DoubleClick { .. } = ev {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok((tray, items))
}

/// Keeps the tray icon, tooltip and menu in step with the node.
fn tray_updates(app: AppHandle, tray: TrayIcon, items: TrayItems) {
    std::thread::spawn(move || {
        let sup = app.state::<AppState>().sup.clone();
        let mut last_color = "";
        let mut last_lang = sup.settings().language;
        loop {
            let lang = sup.settings().language;
            if lang != last_lang {
                for (item, key) in &items.labelled {
                    item.set_text(tr(&lang, key)).ok();
                }
                last_lang = lang;
            }
            let s = sup.status();
            if s.color != last_color {
                tray.set_icon(Some(status_icon(s.color))).ok();
                last_color = s.color;
            }
            tray.set_tooltip(Some(format!("Graphene Node — {}", s.summary))).ok();
            items.status.set_text(format!("● {}", s.summary)).ok();
            let running = s.pid.is_some();
            items.start.set_enabled(!running && s.phase != Phase::Stopping).ok();
            items.stop.set_enabled(running || matches!(s.phase, Phase::WaitingRestart | Phase::Failed)).ok();
            items.restart.set_enabled(running).ok();
            std::thread::sleep(Duration::from_secs(1));
        }
    });
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main(app)))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let settings_path = app.path().app_config_dir()?.join("settings.json");
            let settings = Settings::load(&settings_path);
            let start_now = settings.start_node_with_app;
            let sup = Supervisor::start_new(settings);
            if start_now && !sup.is_node_running() {
                sup.start().ok(); // a missing node shows up in the window's status
            }
            app.manage(AppState { sup, settings_path });
            let lang = app.state::<AppState>().sup.settings().language;
            let (tray, items) = build_tray(app.handle(), &lang)?;
            tray_updates(app.handle().clone(), tray, items);
            show_main(app.handle());
            Ok(())
        })
        .on_window_event(|window, event| {
            // The window's close button asks whether to stop the node or keep it running in the tray
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                ask_on_close(window.app_handle().clone());
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            get_log,
            get_languages,
            node_start,
            node_stop,
            node_restart,
            node_kill,
            get_settings,
            save_settings,
            open_data_dir,
            open_log,
            copy_rpc
        ])
        .run(tauri::generate_context!())
        .expect("error while running the app");
}
