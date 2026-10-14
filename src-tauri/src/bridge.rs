//! Cliente do programa `hon-bridge`, que fala com o hOn.
//!
//! O programa é arrancado uma vez e mantém a sessão do hOn aberta. Cada pedido é
//! uma linha JSON no stdin e a resposta é uma linha JSON no stdout.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{json, Value};

pub struct Bridge {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

/// O executável fica ao lado do da app (o Tauri copia-o para lá).
fn bridge_path() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe.parent().ok_or("sem pasta da app")?;
    let name = if cfg!(windows) { "hon-bridge.exe" } else { "hon-bridge" };
    Ok(dir.join(name))
}

impl Bridge {
    pub fn start() -> Result<Self, String> {
        Self::start_with(Command::new(bridge_path()?))
    }

    pub fn start_with(mut command: Command) -> Result<Self, String> {
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = command
            .spawn()
            .map_err(|e| format!("não consegui arrancar o hon-bridge: {e}"))?;
        let stdin = child.stdin.take().ok_or("sem stdin")?;
        let stdout = BufReader::new(child.stdout.take().ok_or("sem stdout")?);
        Ok(Self { child, stdin, stdout, next_id: 1 })
    }

    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Envia um pedido e espera pela resposta. Devolve `result` ou o código de erro.
    pub fn request(&mut self, cmd: &str, mut args: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        args["id"] = json!(id);
        args["cmd"] = json!(cmd);
        writeln!(self.stdin, "{args}").map_err(|_| "bridge_closed".to_string())?;
        self.stdin.flush().map_err(|_| "bridge_closed".to_string())?;
        loop {
            let mut line = String::new();
            let read = self.stdout.read_line(&mut line).map_err(|e| e.to_string())?;
            if read == 0 {
                return Err("bridge_closed".into());
            }
            let Ok(response) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if response["id"] != json!(id) {
                continue;
            }
            return if response["ok"] == json!(true) {
                Ok(response["result"].clone())
            } else {
                Err(response["error"].as_str().unwrap_or("erro").to_string())
            };
        }
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Um "bridge" de mentira em shell que responde sempre ok com o pedido recebido.
    #[cfg(unix)]
    #[test]
    fn request_matches_response_by_id() {
        let mut cmd = Command::new("sh");
        cmd.args([
            "-c",
            r#"echo 'lixo que não é json'; while read l; do id=$(echo "$l" | sed 's/.*"id":\([0-9]*\).*/\1/'); echo "{\"id\":$id,\"ok\":true,\"result\":\"pong\"}"; done"#,
        ]);
        let mut b = Bridge::start_with(cmd).unwrap();
        assert_eq!(b.request("ping", json!({})).unwrap(), json!("pong"));
        assert_eq!(b.request("ping", json!({})).unwrap(), json!("pong"));
    }

    #[cfg(unix)]
    #[test]
    fn error_response_becomes_err() {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", r#"read l; echo '{"id":1,"ok":false,"error":"auth_failed"}'"#]);
        let mut b = Bridge::start_with(cmd).unwrap();
        assert_eq!(b.request("login", json!({})).unwrap_err(), "auth_failed");
    }
}
