//! Ícone da bandeja: quadrado com a cor do modo e a temperatura escolhida quando
//! o AC está ligado, cinzento com o símbolo de ligar quando está desligado.

use serde_json::Value;
use tauri::{image::Image, AppHandle};

const SIZE: u32 = 64;
const RADIUS: f32 = 14.0;

/// Máscaras brancas (64×64) desenhadas por cima do quadrado colorido.
fn glyph(temp: Option<i64>) -> &'static [u8] {
    match temp {
        Some(16) => include_bytes!("../icons/tray/16.png"),
        Some(17) => include_bytes!("../icons/tray/17.png"),
        Some(18) => include_bytes!("../icons/tray/18.png"),
        Some(19) => include_bytes!("../icons/tray/19.png"),
        Some(20) => include_bytes!("../icons/tray/20.png"),
        Some(21) => include_bytes!("../icons/tray/21.png"),
        Some(22) => include_bytes!("../icons/tray/22.png"),
        Some(23) => include_bytes!("../icons/tray/23.png"),
        Some(24) => include_bytes!("../icons/tray/24.png"),
        Some(25) => include_bytes!("../icons/tray/25.png"),
        Some(26) => include_bytes!("../icons/tray/26.png"),
        Some(27) => include_bytes!("../icons/tray/27.png"),
        Some(28) => include_bytes!("../icons/tray/28.png"),
        Some(29) => include_bytes!("../icons/tray/29.png"),
        Some(30) => include_bytes!("../icons/tray/30.png"),
        _ => include_bytes!("../icons/tray/off.png"),
    }
}

fn mode_color(mode: &str) -> [u8; 3] {
    match mode {
        "heat" => [0xf9, 0x73, 0x16],
        "auto" => [0x8b, 0x5c, 0xf6],
        "dry" => [0x14, 0xb8, 0xa6],
        "fan" => [0x64, 0x74, 0x8b],
        _ => [0x3b, 0x82, 0xf6],
    }
}

fn mode_name(mode: &str) -> &'static str {
    match mode {
        "heat" => "Calor",
        "auto" => "Auto",
        "dry" => "Desumidificar",
        "fan" => "Ventilar",
        _ => "Frio",
    }
}

/// Cobertura (0..1) do quadrado de cantos redondos no píxel (x, y), com antialiasing.
fn coverage(x: u32, y: u32) -> f32 {
    let size = SIZE as f32;
    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
    let cx = px.clamp(RADIUS, size - RADIUS);
    let cy = py.clamp(RADIUS, size - RADIUS);
    let dist = ((px - cx).powi(2) + (py - cy).powi(2)).sqrt();
    (RADIUS - dist + 0.5).clamp(0.0, 1.0)
}

fn render(color: [u8; 3], temp: Option<i64>) -> Option<Image<'static>> {
    let mask = Image::from_bytes(glyph(temp)).ok()?;
    let mask = mask.rgba();
    let mut rgba = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let i = ((y * SIZE + x) * 4) as usize;
            let white = mask.get(i + 3).copied().unwrap_or(0) as f32 / 255.0;
            for c in color {
                rgba.push((c as f32 * (1.0 - white) + 255.0 * white).round() as u8);
            }
            rgba.push((coverage(x, y) * 255.0).round() as u8);
        }
    }
    Some(Image::new_owned(rgba, SIZE, SIZE))
}

/// Atualiza o ícone e a dica da bandeja com o estado devolvido pelo hon-bridge.
pub fn show_state(app: &AppHandle, state: &Value) {
    let Some(tray) = app.tray_by_id("main") else {
        return;
    };
    let name = state["name"].as_str().unwrap_or("AC");
    let mode = state["mode"].as_str().unwrap_or("cool");
    let temp = state["targetTemp"].as_i64();
    let on = state["power"].as_bool().unwrap_or(false);
    let (icon, tooltip) = if on {
        (
            render(mode_color(mode), temp),
            format!("{name} · {}°C · {}", temp.unwrap_or(0), mode_name(mode)),
        )
    } else {
        (render([0x6b, 0x72, 0x80], None), format!("{name} · Desligado"))
    };
    if let Some(icon) = icon {
        let _ = tray.set_icon(Some(icon));
    }
    let _ = tray.set_tooltip(Some(tooltip));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_render_for_every_temperature() {
        for t in 16..=30 {
            assert!(render([0, 0, 255], Some(t)).is_some());
        }
        let off = render([0, 0, 0], None).unwrap();
        assert_eq!(off.rgba().len(), (SIZE * SIZE * 4) as usize);
        // Os cantos ficam transparentes e o centro opaco.
        assert_eq!(off.rgba()[3], 0);
        assert_eq!(off.rgba()[((32 * SIZE + 32) * 4 + 3) as usize], 255);
    }
}
