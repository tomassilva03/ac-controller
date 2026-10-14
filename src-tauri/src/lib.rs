mod bridge;
mod session;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;
use session::Session;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, PhysicalPosition, PhysicalSize, Rect, State, WebviewWindow, WindowEvent,
};

struct AppState {
    session: Arc<Mutex<Session>>,
    /// Quando o painel foi escondido por perder o foco. Clicar no ícone da bandeja
    /// tira o foco ao painel antes do clique chegar, por isso ignoramos o clique
    /// que vem logo a seguir, senão o painel reabria.
    last_hidden: Mutex<Option<Instant>>,
}

/// Posição do ícone da bandeja em píxeis físicos.
struct TrayRect {
    position: PhysicalPosition<f64>,
    size: PhysicalSize<f64>,
}

/// Corre `f` com a sessão numa thread à parte, para os pedidos ao hOn não
/// bloquearem a interface.
async fn with_session<T: Send + 'static>(
    state: State<'_, AppState>,
    f: impl FnOnce(&mut Session) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let session = state.session.clone();
    tauri::async_runtime::spawn_blocking(move || f(&mut session.lock().unwrap()))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn status(state: State<'_, AppState>) -> Result<Value, String> {
    with_session(state, |s| Ok(s.status())).await
}

#[tauri::command]
async fn login(email: String, password: String, state: State<'_, AppState>) -> Result<Value, String> {
    with_session(state, move |s| s.login(&email, &password)).await
}

#[tauri::command]
async fn logout(state: State<'_, AppState>) -> Result<(), String> {
    with_session(state, |s| {
        s.logout();
        Ok(())
    })
    .await
}

#[tauri::command]
async fn select_ac(id: String, state: State<'_, AppState>) -> Result<Value, String> {
    with_session(state, move |s| Ok(s.select(&id))).await
}

#[tauri::command]
async fn get_state(state: State<'_, AppState>) -> Result<Value, String> {
    with_session(state, |s| s.get_state()).await
}

#[tauri::command]
async fn update_state(patch: Value, state: State<'_, AppState>) -> Result<Value, String> {
    with_session(state, move |s| s.update_state(patch)).await
}

#[tauri::command]
fn hide_panel(app: AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
}

fn toggle_panel(app: &AppHandle, tray: Rect) {
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
    let scale = window.scale_factor().unwrap_or(1.0);
    show_panel(
        &window,
        TrayRect {
            position: tray.position.to_physical(scale),
            size: tray.size.to_physical(scale),
        },
    );
}

/// Mostra o painel colado ao ícone: por cima quando a barra está em baixo
/// (Windows), por baixo quando está em cima (macOS), sem sair do ecrã.
fn show_panel(window: &WebviewWindow, tray: TrayRect) {
    let center_x = tray.position.x + tray.size.width / 2.0;
    let center_y = tray.position.y + tray.size.height / 2.0;
    if let (Ok(Some(monitor)), Ok(win)) = (
        window.monitor_from_point(center_x, center_y),
        window.outer_size(),
    ) {
        let area = monitor.work_area();
        let (left, top) = (area.position.x as f64, area.position.y as f64);
        let right = left + area.size.width as f64;
        let bottom = top + area.size.height as f64;
        let (w, h) = (win.width as f64, win.height as f64);

        let x = (center_x - w / 2.0).clamp(left, (right - w).max(left));
        let screen_middle = monitor.position().y as f64 + monitor.size().height as f64 / 2.0;
        let y = if center_y > screen_middle {
            bottom - h
        } else {
            top.max(tray.position.y + tray.size.height)
        };
        let _ = window.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
    }
    let _ = window.show();
    let _ = window.set_focus();
}

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            status,
            login,
            logout,
            select_ac,
            get_state,
            update_state,
            hide_panel
        ])
        .setup(|app| {
            let config_dir = app.path().app_config_dir()?;
            let session = Arc::new(Mutex::new(Session::new(config_dir)));
            // Entra logo no hOn com as credenciais guardadas, sem esperar pelo painel.
            let background = session.clone();
            std::thread::spawn(move || {
                let _ = background.lock().unwrap().auto_login();
            });
            app.manage(AppState { session, last_hidden: Mutex::new(None) });

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
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        rect,
                        ..
                    } = event
                    {
                        toggle_panel(tray.app_handle(), rect);
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
