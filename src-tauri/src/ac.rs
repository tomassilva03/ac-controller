//! Estado do ar condicionado.
//!
//! Por agora é um AC simulado em memória. No passo 3 isto passa a falar com o hOn,
//! mantendo a mesma forma de `AcState` para a interface não mudar.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Auto,
    Cool,
    Dry,
    Heat,
    Fan,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FanSpeed {
    Auto,
    Low,
    Mid,
    High,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Airflow {
    Fixed,
    Swing,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcState {
    pub name: String,
    pub power: bool,
    pub mode: Mode,
    pub target_temp: u8,
    pub room_temp: f32,
    pub fan_speed: FanSpeed,
    pub eco: bool,
    pub sleep: bool,
    pub airflow: Airflow,
}

pub const MIN_TEMP: u8 = 16;
pub const MAX_TEMP: u8 = 30;

impl Default for AcState {
    fn default() -> Self {
        Self {
            name: "Tomás AC".into(),
            power: false,
            mode: Mode::Cool,
            target_temp: 22,
            room_temp: 25.0,
            fan_speed: FanSpeed::Auto,
            eco: false,
            sleep: false,
            airflow: Airflow::Swing,
        }
    }
}

/// Alterações parciais enviadas pela interface.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcPatch {
    pub power: Option<bool>,
    pub mode: Option<Mode>,
    pub target_temp: Option<u8>,
    pub fan_speed: Option<FanSpeed>,
    pub eco: Option<bool>,
    pub sleep: Option<bool>,
    pub airflow: Option<Airflow>,
}

impl AcState {
    pub fn apply(&mut self, patch: AcPatch) {
        if let Some(v) = patch.power {
            self.power = v;
        }
        if let Some(v) = patch.mode {
            self.mode = v;
        }
        if let Some(v) = patch.target_temp {
            self.target_temp = v.clamp(MIN_TEMP, MAX_TEMP);
        }
        if let Some(v) = patch.fan_speed {
            self.fan_speed = v;
        }
        if let Some(v) = patch.eco {
            self.eco = v;
        }
        if let Some(v) = patch.sleep {
            self.sleep = v;
        }
        if let Some(v) = patch.airflow {
            self.airflow = v;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_changes_only_given_fields_and_clamps_temp() {
        let mut s = AcState::default();
        s.apply(AcPatch {
            power: Some(true),
            target_temp: Some(40),
            ..Default::default()
        });
        assert!(s.power);
        assert_eq!(s.target_temp, MAX_TEMP);
        assert_eq!(s.mode, Mode::Cool);
    }

    #[test]
    fn patch_parses_from_interface_json() {
        let p: AcPatch = serde_json::from_str(r#"{"mode":"heat","fanSpeed":"low"}"#).unwrap();
        assert_eq!(p.mode, Some(Mode::Heat));
        assert_eq!(p.fan_speed, Some(FanSpeed::Low));
    }
}
