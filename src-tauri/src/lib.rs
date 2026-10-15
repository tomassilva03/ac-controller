mod bridge;
mod session;
mod timer;
mod tray;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;
use session::Session;
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, PhysicalPosition, PhysicalSize, Rect, State, WebviewWindow, WindowEvent,
};

struct AppState {
    session: Arc<Mutex<Session>>,
    /// Quando o painel foi escondido por perder o foco. Clicar no ícone da bandeja
    /// tira o foco ao painel antes do clique chegar, por isso ignoramos o clique
    /// que vem logo a seguir, senão o painel reabria.
    last_hidden: Mutex<Option<Instant>>,
    timer: timer::Shared,
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
async fn get_state(app: AppHandle, state: State<'_, AppState>) -> Result<Value, String> {
    let result = with_session(state, |s| s.get_state()).await?;
    tray::show_state(&app, &result);
    Ok(result)
}

#[tauri::command]
async fn update_state(
    patch: Value,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    if patch["power"] == Value::Bool(false) {
        *state.timer.lock().unwrap() = None;
    }
    let result = with_session(state, move |s| s.update_state(patch)).await?;
    tray::show_state(&app, &result);
    Ok(result)
}

/// Temporizador do AC escolhido: `null` ou `{ ac, deadline }` (milissegundos).
#[tauri::command]
async fn get_timer(state: State<'_, AppState>) -> Result<Value, String> {
    let selected = state.session.lock().unwrap().selected_id();
    Ok(match state.timer.lock().unwrap().as_ref() {
        Some(t) if Some(&t.ac) == selected.as_ref() => t.to_json(),
        _ => Value::Null,
    })
}

/// Desliga o AC escolhido daqui a `minutes` minutos; `None` cancela.
#[tauri::command]
async fn set_timer(minutes: Option<u64>, state: State<'_, AppState>) -> Result<Value, String> {
    let Some(minutes) = minutes else {
        *state.timer.lock().unwrap() = None;
        return Ok(Value::Null);
    };
    // A sessão pode estar ocupada com um pedido ao hOn; lê-se antes de bloquear o temporizador.
    let ac = state.session.lock().unwrap().selected_id().ok_or("no_ac")?;
    let mut timer = state.timer.lock().unwrap();
    let t = timer::Timer {
        ac,
        deadline: std::time::SystemTime::now() + Duration::from_secs(minutes * 60),
    };
    let json = t.to_json();
    *timer = Some(t);
    Ok(json)
}

#[tauri::command]
fn hide_panel(app: AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
}

/// Procura uma versão nova no GitHub; se houver, instala-a e reinicia a app.
async fn check_for_update(app: AppHandle) -> tauri_plugin_updater::Result<()> {
    use tauri_plugin_updater::UpdaterExt;
    if let Some(update) = app.updater()?.check().await? {
        update.download_and_install(|_, _| {}, || {}).await?;
        app.restart();
    }
    Ok(())
}

#[tauri::command]
fn app_version(app: AppHandle) -> String {
    app.package_info().version.to_string()
}

/// Botão do painel: devolve a versão nova (que fica a instalar-se em fundo e
/// reinicia a app) ou `None` se já estiver na mais recente.
#[tauri::command]
async fn check_update(app: AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_updater::UpdaterExt;
    let update = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?;
    let Some(update) = update else {
        return Ok(None);
    };
    let version = update.version.clone();
    tauri::async_runtime::spawn(async move {
        if update.download_and_install(|_, _| {}, || {}).await.is_ok() {
            app.restart();
        }
    });
    Ok(Some(version))
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
    show_panel(&window, tray);
}

/// Valores "crus" de uma posição/tamanho, seja qual for a unidade.
fn raw_pos(p: tauri::Position) -> (f64, f64) {
    match p {
        tauri::Position::Physical(p) => (p.x as f64, p.y as f64),
        tauri::Position::Logical(p) => (p.x, p.y),
    }
}

fn raw_size(s: tauri::Size) -> (f64, f64) {
    match s {
        tauri::Size::Physical(s) => (s.width as f64, s.height as f64),
        tauri::Size::Logical(s) => (s.width, s.height),
    }
}

/// Encontra o ecrã onde está o ícone e devolve-o com o retângulo do ícone em
/// píxeis físicos. Consoante o sistema, o retângulo vem em píxeis físicos ou
/// lógicos (no Mac com ecrã Retina são o dobro), por isso testam-se as duas
/// leituras e fica a que cai dentro de um ecrã.
fn locate_tray(window: &WebviewWindow, tray: Rect) -> Option<(tauri::Monitor, TrayRect)> {
    // Primeiro a unidade que o Tauri diz; depois a outra.
    let logical = matches!(tray.position, tauri::Position::Logical(_));
    let (x, y) = raw_pos(tray.position);
    let (w, h) = raw_size(tray.size);
    for monitor in window.available_monitors().ok()? {
        let scale = monitor.scale_factor();
        let (mx, my) = (monitor.position().x as f64, monitor.position().y as f64);
        let (mw, mh) = (monitor.size().width as f64, monitor.size().height as f64);
        let factors = if logical { [scale, 1.0] } else { [1.0, scale] };
        for k in factors {
            let (cx, cy) = ((x + w / 2.0) * k, (y + h / 2.0) * k);
            if cx >= mx && cx < mx + mw && cy >= my && cy < my + mh {
                let rect = TrayRect {
                    position: PhysicalPosition::new(x * k, y * k),
                    size: PhysicalSize::new(w * k, h * k),
                };
                return Some((monitor, rect));
            }
        }
    }
    None
}

/// Mostra o painel colado ao ícone: por cima quando a barra está em baixo
/// (Windows), por baixo quando está em cima (macOS), sem sair do ecrã. Se não
/// der para saber onde está o ícone, fica no canto do ecrã principal onde
/// costumam estar os ícones (em cima à direita no Mac, em baixo no Windows).
fn show_panel(window: &WebviewWindow, tray: Rect) {
    let located = locate_tray(window, tray).or_else(|| {
        let monitor = window.primary_monitor().ok()??;
        let area = monitor.work_area();
        let x = area.position.x as f64 + area.size.width as f64 - 40.0;
        let y = if cfg!(target_os = "macos") {
            area.position.y as f64 - 1.0
        } else {
            area.position.y as f64 + area.size.height as f64
        };
        let rect = TrayRect { position: PhysicalPosition::new(x, y), size: PhysicalSize::new(1.0, 1.0) };
        Some((monitor, rect))
    });
    if let (Some((monitor, tray)), Ok(win)) = (located, window.outer_size()) {
        let center_x = tray.position.x + tray.size.width / 2.0;
        let center_y = tray.position.y + tray.size.height / 2.0;
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
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .invoke_handler(tauri::generate_handler![
            status,
            login,
            logout,
            select_ac,
            get_state,
            update_state,
            hide_panel,
            app_version,
            check_update,
            get_timer,
            set_timer
        ])
        .setup(|app| {
            let config_dir = app.path().app_config_dir()?;
            let log_dir = app.path().app_log_dir()?;
            let session = Arc::new(Mutex::new(Session::new(config_dir, log_dir)));
            // Entra logo no hOn com as credenciais guardadas, sem esperar pelo painel.
            // Depois vai lendo o estado de 2 em 2 minutos para o ícone da bandeja
            // estar sempre certo, mesmo com o painel fechado.
            let background = session.clone();
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let _ = background.lock().unwrap().auto_login();
                loop {
                    let state = background.lock().unwrap().get_state();
                    if let Ok(state) = state {
                        tray::show_state(&handle, &state);
                    }
                    std::thread::sleep(Duration::from_secs(120));
                }
            });
            let shared_timer: timer::Shared = Arc::new(Mutex::new(None));
            timer::start(app.handle().clone(), shared_timer.clone(), session.clone());
            app.manage(AppState { session, last_hidden: Mutex::new(None), timer: shared_timer });

            // Procura atualizações ao arrancar e depois de 6 em 6 horas.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    let _ = check_for_update(handle.clone()).await;
                    let _ = tauri::async_runtime::spawn_blocking(|| {
                        std::thread::sleep(Duration::from_secs(6 * 60 * 60))
                    })
                    .await;
                }
            });

            // No macOS a app vive só na barra de menus, sem ícone na Dock.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            // Na primeira vez que a app corre fica a arrancar com o sistema; depois
            // quem manda é a opção do menu.
            let autostart = app.autolaunch();
            let marker = app.path().app_config_dir()?.join("autostart-configured");
            if !marker.exists() {
                let _ = autostart.enable();
                let _ = std::fs::create_dir_all(marker.parent().unwrap());
                let _ = std::fs::write(&marker, "");
            }
            let startup = CheckMenuItem::with_id(
                app,
                "startup",
                "Arrancar com o sistema",
                true,
                autostart.is_enabled().unwrap_or(false),
                None::<&str>,
            )?;
            let quit = MenuItem::with_id(app, "quit", "Sair", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&startup, &quit])?;

            TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("AC Controller")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(move |app, event| match event.id.as_ref() {
                    "startup" => {
                        let autostart = app.autolaunch();
                        let enabled = autostart.is_enabled().unwrap_or(false);
                        let _ = if enabled { autostart.disable() } else { autostart.enable() };
                        let _ = startup.set_checked(autostart.is_enabled().unwrap_or(!enabled));
                    }
                    "quit" => app.exit(0),
                    _ => {}
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
