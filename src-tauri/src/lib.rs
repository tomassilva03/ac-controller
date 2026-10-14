mod ac;

use std::sync::Mutex;
use std::time::{Duration, Instant};

use ac::{AcPatch, AcState};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, State, WebviewWindow, WindowEvent,
};
use tauri_plugin_positioner::{Position, WindowExt};

struct AppState {
    ac: Mutex<AcState>,
    /// Quando o painel foi escondido por perder o foco. Clicar no ícone da bandeja
    /// tira o foco ao painel antes do clique chegar, por isso ignoramos o clique
    /// que vem logo a seguir, senão o painel reabria.
    last_hidden: Mutex<Option<Instant>>,
}

#[tauri::command]
fn get_state(state: State<AppState>) -> AcState {
    state.ac.lock().unwrap().clone()
}

#[tauri::command]
fn update_state(patch: AcPatch, state: State<AppState>) -> AcState {
    let mut ac = state.ac.lock().unwrap();
    ac.apply(patch);
    ac.clone()
}

#[tauri::command]
fn hide_panel(app: AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
}

#[cfg(target_os = "macos")]
const PANEL_POSITION: Position = Position::TrayCenter;
#[cfg(not(target_os = "macos"))]
const PANEL_POSITION: Position = Position::TrayBottomCenter;

fn toggle_panel(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
        return;
    }
    let state = app.state::<AppState>();
    let just_hidden = state
        .last_hidden
        .lock()
        .unwrap()
        .is_some_and(|t| t.elapsed() < Duration::from_millis(250));
    if just_hidden {
        return;
    }
    show_panel(&window);
}

fn show_panel(window: &WebviewWindow) {
    let _ = window.move_window(PANEL_POSITION);
    let _ = window.show();
    let _ = window.set_focus();
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_positioner::init())
        .manage(AppState {
            ac: Mutex::new(AcState::default()),
            last_hidden: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![get_state, update_state, hide_panel])
        .setup(|app| {
            // No macOS a app vive só na barra de menus, sem ícone na Dock.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let quit = MenuItem::with_id(app, "quit", "Sair", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&quit])?;

            TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("AC Controller")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| {
                    if event.id.as_ref() == "quit" {
                        app.exit(0);
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    tauri_plugin_positioner::on_tray_event(tray.app_handle(), &event);
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        toggle_panel(tray.app_handle());
                    }
                })
                .build(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::Focused(false) = event {
                let _ = window.hide();
                let state = window.app_handle().state::<AppState>();
                *state.last_hidden.lock().unwrap() = Some(Instant::now());
            }
        })
        .run(tauri::generate_context!())
        .expect("erro ao arrancar a app");
}
