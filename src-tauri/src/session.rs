//! Sessão do hOn: credenciais, AC escolhido e pedidos ao `hon-bridge`.
//!
//! O email e o AC escolhido ficam num ficheiro de configuração; a password fica no
//! cofre do sistema (Credential Manager no Windows, Keychain no macOS).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::bridge::Bridge;

const KEYRING_SERVICE: &str = "ac-controller-hon";

#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Config {
    email: Option<String>,
    selected_ac: Option<String>,
}

pub struct Session {
    config_path: PathBuf,
    log_file: PathBuf,
    config: Config,
    bridge: Option<Bridge>,
    acs: Vec<Value>,
    logged_in: bool,
}

impl Session {
    pub fn new(config_dir: PathBuf, log_dir: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&log_dir);
        let log_file = log_dir.join("hon-bridge.log");
        let config_path = config_dir.join("config.json");
        let config = std::fs::read_to_string(&config_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self { config_path, log_file, config, bridge: None, acs: vec![], logged_in: false }
    }

    fn save_config(&self) {
        if let Some(dir) = self.config_path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string_pretty(&self.config) {
            let _ = std::fs::write(&self.config_path, text);
        }
    }

    fn saved_password(&self) -> Option<String> {
        let email = self.config.email.as_ref()?;
        keyring::Entry::new(KEYRING_SERVICE, email).ok()?.get_password().ok()
    }

    pub fn status(&self) -> Value {
        json!({
            "loggedIn": self.logged_in,
            "email": self.config.email,
            "acs": self.acs,
            "selected": self.config.selected_ac,
        })
    }

    /// Entra com as credenciais guardadas, se existirem.
    pub fn auto_login(&mut self) -> Result<(), String> {
        let (Some(email), Some(password)) = (self.config.email.clone(), self.saved_password())
        else {
            return Err("no_saved_login".into());
        };
        self.login_inner(&email, &password)
    }

    pub fn login(&mut self, email: &str, password: &str) -> Result<Value, String> {
        self.login_inner(email, password)?;
        if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, email) {
            let _ = entry.set_password(password);
        }
        if self.config.email.as_deref() != Some(email) {
            self.config.email = Some(email.to_string());
            self.save_config();
        }
        Ok(self.status())
    }

    fn login_inner(&mut self, email: &str, password: &str) -> Result<(), String> {
        let bridge = self.bridge()?;
        let acs = bridge.request("login", json!({ "email": email, "password": password }))?;
        self.acs = acs.as_array().cloned().unwrap_or_default();
        self.logged_in = true;
        let ids: Vec<&str> = self.acs.iter().filter_map(|a| a["id"].as_str()).collect();
        let selected_ok = self.config.selected_ac.as_deref().is_some_and(|s| ids.contains(&s));
        if !selected_ok {
            self.config.selected_ac = ids.first().map(|s| s.to_string());
            self.save_config();
        }
        Ok(())
    }

    pub fn logout(&mut self) {
        if let Some(email) = &self.config.email {
            if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, email) {
                let _ = entry.delete_credential();
            }
        }
        self.bridge = None;
        self.acs.clear();
        self.logged_in = false;
        self.config = Config::default();
        self.save_config();
    }

    pub fn select(&mut self, id: &str) -> Value {
        if self.acs.iter().any(|a| a["id"] == json!(id)) {
            self.config.selected_ac = Some(id.to_string());
            self.save_config();
        }
        self.status()
    }

    pub fn get_state(&mut self) -> Result<Value, String> {
        let ac = self.selected()?;
        self.call("state", json!({ "ac": ac }))
    }

    pub fn update_state(&mut self, patch: Value) -> Result<Value, String> {
        let ac = self.selected()?;
        self.update_ac(&ac, patch)
    }

    /// Muda um AC em concreto (o temporizador usa isto, porque o AC escolhido
    /// no painel pode já ser outro quando chega a hora).
    pub fn update_ac(&mut self, ac: &str, patch: Value) -> Result<Value, String> {
        if !self.logged_in {
            return Err("not_logged_in".into());
        }
        self.call("set", json!({ "ac": ac, "patch": patch }))
    }

    pub fn selected_id(&self) -> Option<String> {
        self.config.selected_ac.clone()
    }

    fn selected(&self) -> Result<String, String> {
        if !self.logged_in {
            return Err("not_logged_in".into());
        }
        self.config.selected_ac.clone().ok_or_else(|| "no_ac".into())
    }

    fn bridge(&mut self) -> Result<&mut Bridge, String> {
        if !self.bridge.as_mut().is_some_and(|b| b.is_alive()) {
            self.bridge = Some(Bridge::start(&self.log_file)?);
        }
        Ok(self.bridge.as_mut().unwrap())
    }

    /// Faz o pedido; se o hon-bridge tiver morrido ou perdido a sessão, volta a
    /// entrar com as credenciais guardadas e tenta outra vez.
    fn call(&mut self, cmd: &str, args: Value) -> Result<Value, String> {
        match self.bridge()?.request(cmd, args.clone()) {
            Err(e) if e == "bridge_closed" || e == "not_logged_in" => {
                self.bridge = None;
                self.auto_login()?;
                self.bridge()?.request(cmd, args)
            }
            other => other,
        }
    }
}
