mod landscape;
mod launcher;
mod systems;

use std::{
    collections::BTreeMap,
    net::{TcpStream, ToSocketAddrs},
    path::PathBuf,
    sync::{mpsc, Mutex},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use systems::{Logon, Store, System};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem, Submenu},
    plugin::{self, TauriPlugin},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager, RunEvent, WindowEvent, Wry,
};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_dialog::DialogExt;

struct AppState {
    path: PathBuf,
    store: Mutex<Store>,
    /// The password on the clipboard that is still to be cleared.
    copied_secret: Mutex<Option<String>>,
}

type CmdResult<T> = Result<T, String>;

const TRAY_ID: &str = "main";
const RECENT_IN_TRAY: usize = 5;
const CLEAR_SECRET_AFTER: Duration = Duration::from_secs(30);

fn store_of(app: &AppHandle) -> CmdResult<Store> {
    Ok(app.state::<AppState>().store.lock().map_err(|e| e.to_string())?.clone())
}

fn update(app: &AppHandle, change: impl FnOnce(&mut Store)) -> CmdResult<Store> {
    try_update(app, |s| {
        change(s);
        Ok(())
    })
}

fn try_update(app: &AppHandle, change: impl FnOnce(&mut Store) -> CmdResult<()>) -> CmdResult<Store> {
    let state = app.state::<AppState>();
    let mut store = state.store.lock().map_err(|e| e.to_string())?;
    let mut next = store.clone();
    change(&mut next)?;
    next.save(&state.path).map_err(|e| format!("Could not save {}: {e}", state.path.display()))?;
    *store = next.clone();
    drop(store);
    refresh_tray(app, &next);
    Ok(next)
}

fn launch_and_record(app: &AppHandle, id: &str, logon_id: &str) -> CmdResult<Store> {
    let store = store_of(app)?;
    let system = store.systems.iter().find(|s| s.id == id).ok_or("System not found.")?;
    let logon = system.logons.iter().find(|l| l.id == logon_id).ok_or("User not found.")?;
    launcher::launch(&store.sapgui_path, system, logon)?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or_default();
    update(app, |s| {
        if let Some(x) = s.systems.iter_mut().find(|x| x.id == id) {
            x.last_used = now;
        }
    })
}

#[tauri::command]
fn get_store(app: AppHandle) -> CmdResult<Store> {
    store_of(&app)
}

#[tauri::command]
fn save_system(system: System, app: AppHandle) -> CmdResult<Store> {
    if system.id.is_empty() {
        return Err("System id is empty.".into());
    }
    update(&app, |s| s.upsert(system))
}

#[tauri::command]
fn delete_system(id: String, app: AppHandle) -> CmdResult<Store> {
    update(&app, |s| s.systems.retain(|x| x.id != id))
}

#[tauri::command]
fn add_folder(path: Vec<String>, app: AppHandle) -> CmdResult<Store> {
    try_update(&app, |s| s.add_folder(path))
}

#[tauri::command]
fn move_folder(from: Vec<String>, to: Vec<String>, app: AppHandle) -> CmdResult<Store> {
    try_update(&app, |s| s.move_folder(&from, to))
}

#[tauri::command]
fn delete_folder(path: Vec<String>, app: AppHandle) -> CmdResult<Store> {
    try_update(&app, |s| s.delete_folder(&path))
}

#[tauri::command]
fn set_favorite(id: String, favorite: bool, app: AppHandle) -> CmdResult<Store> {
    update(&app, |s| {
        if let Some(x) = s.systems.iter_mut().find(|x| x.id == id) {
            x.favorite = favorite;
        }
    })
}

/// Asks for the SAP GUI file with the picker of the OS. None means the user cancelled.
// async, because the blocking picker must not run on the main thread
#[tauri::command]
async fn choose_sapgui(window: tauri::WebviewWindow, app: AppHandle) -> CmdResult<Option<Store>> {
    let mut picker = app.dialog().file().set_parent(&window).set_title(launcher::CHOOSE_TITLE);
    if cfg!(windows) {
        picker = picker.add_filter("sapshcut.exe", &["exe"]);
    } else if cfg!(target_os = "macos") {
        picker = picker.add_filter("SAPGUI", &["app"]);
    }
    if let Some(folder) = launcher::picker_folder(&store_of(&app)?.sapgui_path) {
        picker = picker.set_directory(folder);
    }
    let Some(file) = picker.blocking_pick_file() else {
        return Ok(None);
    };
    let file = file.into_path().map_err(|e| e.to_string())?;
    let exe = launcher::sapgui_from_file(&file)
        .ok_or_else(|| format!("{} is not SAP GUI. {}.", file.display(), launcher::CHOOSE_TITLE))?;
    update(&app, |s| s.sapgui_path = exe.to_string_lossy().into_owned()).map(Some)
}

#[tauri::command]
fn import_landscape(app: AppHandle) -> CmdResult<Store> {
    let path = landscape::default_path().ok_or("Home folder not found.")?;
    let xml = std::fs::read_to_string(&path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => format!("There is no SAP Logon file at {}. Add your systems by hand.", path.display()),
        _ => format!("Could not read {}: {e}", path.display()),
    })?;
    let imported = landscape::parse(&xml).map_err(|e| format!("{} is not a valid SAP Logon file: {e}", path.display()))?;
    update(&app, |s| {
        s.merge(imported);
    })
}

#[tauri::command]
fn launch_system(id: String, logon_id: String, app: AppHandle) -> CmdResult<Store> {
    launch_and_record(&app, &id, &logon_id)
}

#[tauri::command]
fn copy_text(text: String, secret: bool, app: AppHandle) -> CmdResult<()> {
    if !secret {
        return app.clipboard().write_text(text).map_err(|e| e.to_string());
    }
    // Asks clipboard managers not to keep it.
    app.clipboard().write_secret(text.clone()).map_err(|e| e.to_string())?;
    *app.state::<AppState>().copied_secret.lock().map_err(|e| e.to_string())? = Some(text.clone());
    thread::spawn(move || {
        thread::sleep(CLEAR_SECRET_AFTER);
        clear_secret(&app, &text);
    });
    Ok(())
}

/// Clears the clipboard, unless something else was copied since.
fn clear_secret(app: &AppHandle, secret: &str) {
    if app.clipboard().read_text().ok().as_deref() == Some(secret) {
        let _ = app.clipboard().clear();
    }
    if let Ok(mut pending) = app.state::<AppState>().copied_secret.lock() {
        pending.take_if(|p| p == secret);
    }
}

/// Clears a copied password when the app quits before its time is up. A plugin, because the clipboard plugin closes
/// the clipboard on exit before the app sees the event: register this one first.
fn clear_secret_on_exit() -> TauriPlugin<Wry> {
    plugin::Builder::new("clear-secret")
        .on_event(|app, event| {
            if !matches!(event, RunEvent::Exit) {
                return;
            }
            let pending = app.try_state::<AppState>().and_then(|s| s.copied_secret.lock().ok().and_then(|p| p.clone()));
            let Some(secret) = pending else { return };
            // Reading the clipboard on the main thread can block on Linux.
            let (done, wait) = mpsc::channel();
            let app = app.clone();
            thread::spawn(move || {
                clear_secret(&app, &secret);
                let _ = done.send(());
            });
            let _ = wait.recv_timeout(Duration::from_secs(1));
        })
        .build()
}

#[tauri::command]
async fn check_connection(host: String, sysnr: String) -> CmdResult<u64> {
    tauri::async_runtime::spawn_blocking(move || {
        let target = format!("{host}:32{sysnr}");
        let addr = target
            .to_socket_addrs()
            .map_err(|e| format!("Host \"{host}\" was not found: {e}"))?
            .next()
            .ok_or(format!("Host \"{host}\" was not found."))?;
        let start = Instant::now();
        TcpStream::connect_timeout(&addr, Duration::from_secs(3)).map_err(|e| format!("No connection to {target}: {e}"))?;
        Ok(start.elapsed().as_millis() as u64)
    })
    .await
    .map_err(|e| e.to_string())?
}

struct TraySystems<'a> {
    favorites: Vec<&'a System>,
    recent: Vec<&'a System>,
    all: Vec<&'a System>,
}

// A system without users has nothing to launch, so the tray leaves it out.
fn tray_systems(store: &Store) -> TraySystems<'_> {
    let mut all: Vec<&System> = store.systems.iter().filter(|s| !s.logons.is_empty()).collect();
    all.sort_by_key(|s| s.name.to_lowercase());
    let favorites = all.iter().copied().filter(|s| s.favorite).collect();
    let mut recent: Vec<&System> = all.iter().copied().filter(|s| !s.favorite && s.last_used > 0).collect();
    recent.sort_by_key(|s| std::cmp::Reverse(s.last_used));
    recent.truncate(RECENT_IN_TRAY);
    TraySystems { favorites, recent, all }
}

fn tray_label(s: &System, l: &Logon) -> String {
    let logon = [&l.client, &l.user, &l.language].into_iter().filter(|x| !x.is_empty()).cloned().collect::<Vec<_>>().join(" ");
    let name = if s.name.is_empty() { &s.host } else { &s.name };
    if logon.is_empty() { name.clone() } else { format!("{name}  ·  {logon}") }
}

fn logon_items(app: &AppHandle, systems: &[&System]) -> tauri::Result<Vec<MenuItem<Wry>>> {
    systems
        .iter()
        .flat_map(|s| s.logons.iter().map(move |l| (*s, l)))
        .map(|(s, l)| MenuItem::with_id(app, format!("launch:{}:{}", s.id, l.id), tray_label(s, l), true, None::<&str>))
        .collect()
}

/// Splits systems into the folders at this depth and the systems directly in it. The key sorts folders without case.
fn split_folders<'a>(systems: &[&'a System], depth: usize) -> (BTreeMap<(String, String), Vec<&'a System>>, Vec<&'a System>) {
    let mut folders: BTreeMap<_, Vec<&System>> = BTreeMap::new();
    let mut direct = Vec::new();
    for &s in systems {
        match s.folder.get(depth) {
            Some(name) => folders.entry((name.to_lowercase(), name.clone())).or_default().push(s),
            None => direct.push(s),
        }
    }
    (folders, direct)
}

fn append_folder(app: &AppHandle, menu: &Submenu<Wry>, systems: &[&System], depth: usize) -> tauri::Result<()> {
    let (folders, direct) = split_folders(systems, depth);
    for ((_, name), inside) in &folders {
        let submenu = Submenu::new(app, name, true)?;
        append_folder(app, &submenu, inside, depth + 1)?;
        menu.append(&submenu)?;
    }
    for item in logon_items(app, &direct)? {
        menu.append(&item)?;
    }
    Ok(())
}

fn tray_menu(app: &AppHandle, store: &Store) -> tauri::Result<Menu<Wry>> {
    let menu = Menu::new(app)?;
    let TraySystems { favorites, recent, all } = tray_systems(store);

    for (title, list) in [("Favorites", &favorites), ("Recent", &recent)] {
        if list.is_empty() {
            continue;
        }
        menu.append(&MenuItem::new(app, title, false, None::<&str>)?)?;
        for item in logon_items(app, list)? {
            menu.append(&item)?;
        }
        menu.append(&PredefinedMenuItem::separator(app)?)?;
    }
    if !all.is_empty() {
        let submenu = Submenu::new(app, "All systems", true)?;
        append_folder(app, &submenu, &all, 0)?;
        menu.append(&submenu)?;
        menu.append(&PredefinedMenuItem::separator(app)?)?;
    }
    menu.append(&MenuItem::with_id(app, "show", "Show window", true, None::<&str>)?)?;
    menu.append(&MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?)?;
    Ok(menu)
}

fn refresh_tray(app: &AppHandle, store: &Store) {
    if let (Some(tray), Ok(menu)) = (app.tray_by_id(TRAY_ID), tray_menu(app, store)) {
        let _ = tray.set_menu(Some(menu));
    }
}

fn show_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(clear_secret_on_exit())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let path = app.path().app_config_dir()?.join("systems.json");
            let mut store = Store::load(&path)?;
            if store.sapgui_path.is_empty() {
                store.sapgui_path = launcher::default_sapgui_path();
            }
            let menu = tray_menu(app.handle(), &store)?;
            app.manage(AppState { path, store: Mutex::new(store), copied_secret: Mutex::default() });

            let mut tray = TrayIconBuilder::with_id(TRAY_ID)
                .tooltip("SAP GUI Launcher")
                .menu(&menu)
                .show_menu_on_left_click(true)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => show_window(app),
                    "quit" => app.exit(0),
                    id => {
                        if let Some((id, logon_id)) = id.strip_prefix("launch:").and_then(|x| x.split_once(':')) {
                            let _ = match launch_and_record(app, id, logon_id) {
                                Ok(store) => app.emit("store", store),
                                Err(e) => {
                                    show_window(app);
                                    app.emit("launch-error", e)
                                }
                            };
                        }
                    }
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                // the window drops passwords that are shown in clear text
                let _ = window.emit("hidden", ());
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_store,
            save_system,
            delete_system,
            add_folder,
            move_folder,
            delete_folder,
            set_favorite,
            choose_sapgui,
            import_landscape,
            launch_system,
            copy_text,
            check_connection
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // macOS: clicking the Dock icon brings the hidden window back.
            #[cfg(target_os = "macos")]
            if let RunEvent::Reopen { .. } = event {
                show_window(app);
            }
            let _ = (app, event);
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_lists_every_system_with_users() {
        let sys = |name: &str, users: usize, favorite: bool, last_used: u64| System {
            name: name.into(),
            logons: vec![Logon::default(); users],
            favorite,
            last_used,
            ..Default::default()
        };
        let store = Store {
            systems: vec![
                sys("b", 2, false, 0),
                sys("A", 1, true, 5),
                sys("c", 1, false, 9),
                sys("d", 0, true, 9),
                sys("e", 0, false, 9),
            ],
            ..Default::default()
        };
        let names = |list: &[&System]| list.iter().map(|s| s.name.clone()).collect::<Vec<_>>().join(" ");
        let tray = tray_systems(&store);
        assert_eq!(names(&tray.favorites), "A");
        assert_eq!(names(&tray.recent), "c");
        assert_eq!(names(&tray.all), "A b c");
    }

    #[test]
    fn tray_groups_systems_by_folder() {
        let sys = |name: &str, folder: &[&str]| System {
            name: name.into(),
            folder: folder.iter().map(|f| f.to_string()).collect(),
            ..Default::default()
        };
        let store = [sys("a", &["Z"]), sys("b", &[]), sys("c", &["y", "N"]), sys("d", &["y"])];
        let names = |list: &[&System]| list.iter().map(|s| s.name.clone()).collect::<Vec<_>>().join(" ");
        let (folders, direct) = split_folders(&store.iter().collect::<Vec<_>>(), 0);
        assert_eq!(names(&direct), "b");
        let got: Vec<_> = folders.iter().map(|((_, f), list)| format!("{f}: {}", names(list))).collect();
        assert_eq!(got, ["y: c d", "Z: a"]);
        let (nested, direct) = split_folders(&folders[&("y".into(), "y".into())], 1);
        assert_eq!(names(&direct), "d");
        assert_eq!(names(&nested[&("n".into(), "N".into())]), "c");
    }

    #[test]
    fn tray_label_shows_client_user_and_language() {
        let s = System { name: "DEV".into(), ..Default::default() };
        let mut l = Logon { client: "100".into(), user: "DEMO".into(), language: "EN".into(), ..Default::default() };
        assert_eq!(tray_label(&s, &l), "DEV  ·  100 DEMO EN");
        l.language.clear();
        assert_eq!(tray_label(&s, &l), "DEV  ·  100 DEMO");
        assert_eq!(tray_label(&s, &Logon::default()), "DEV");
    }
}
