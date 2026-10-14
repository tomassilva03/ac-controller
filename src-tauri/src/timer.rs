//! Temporizador para desligar o AC. O AC não tem temporizador próprio no hOn, por
//! isso a contagem corre aqui: o PC tem de estar ligado quando chega a hora.

use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::session::Session;

#[derive(Clone)]
pub struct Timer {
    pub ac: String,
    /// `SystemTime` (e não `Instant`) para contar também o tempo em suspensão.
    pub deadline: SystemTime,
}

impl Timer {
    pub fn to_json(&self) -> Value {
        let ms = self.deadline.duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
        json!({ "ac": self.ac, "deadline": ms })
    }
}

pub type Shared = Arc<Mutex<Option<Timer>>>;

/// Verifica de 10 em 10 segundos se já está na hora; se o pedido falhar (sem
/// rede, por exemplo) tenta outra vez na volta seguinte.
pub fn start(app: AppHandle, timer: Shared, session: Arc<Mutex<Session>>) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(10));
        let due = {
            let mut t = timer.lock().unwrap();
            match t.as_ref() {
                Some(x) if x.deadline <= SystemTime::now() => t.take(),
                _ => None,
            }
        };
        let Some(due) = due else { continue };
        let result = session.lock().unwrap().update_ac(&due.ac, json!({ "power": false }));
        match result {
            Ok(state) => {
                let selected = session.lock().unwrap().selected_id();
                if selected.as_deref() == Some(due.ac.as_str()) {
                    crate::tray::show_state(&app, &state);
                    let _ = app.emit("ac-state", &state);
                }
                let _ = app.emit("timer", Value::Null);
            }
            Err(_) => {
                timer.lock().unwrap().get_or_insert(due);
            }
        }
    });
}
