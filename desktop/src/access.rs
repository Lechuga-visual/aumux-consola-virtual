//! Optional shared access. Password derivation and sessions are confined to HTTP/control threads.
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    sync::Mutex,
    time::{Duration, Instant},
};
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Disabled,
    Open,
    Password,
}
struct Inner {
    mode: Mode,
    salt: [u8; 16],
    digest: [u8; 32],
    sessions: VecDeque<(String, Instant)>,
    attempts: VecDeque<Instant>,
    generation: u64,
}
pub struct Gate {
    inner: Mutex<Inner>,
}
pub fn secret() -> Result<String, String> {
    let mut bytes = [0u8; 24];
    getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
impl Gate {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                mode: Mode::Disabled,
                salt: [0; 16],
                digest: [0; 32],
                sessions: VecDeque::new(),
                attempts: VecDeque::new(),
                generation: 0,
            }),
        }
    }
    pub fn configure(&self, mode: &str, password: Option<&str>) -> Result<(), String> {
        let mode = match mode {
            "disabled" => Mode::Disabled,
            "open" => Mode::Open,
            "password" => Mode::Password,
            _ => return Err("Modo de acceso inválido".into()),
        };
        let mut salt = [0; 16];
        let mut digest = [0; 32];
        if mode == Mode::Password {
            let password = password.ok_or("Escribe una contraseña")?;
            if password.is_empty() || password.len() > 256 {
                return Err("Contraseña requerida (máximo 256 bytes)".into());
            }
            getrandom::fill(&mut salt).map_err(|e| e.to_string())?;
            pbkdf2::pbkdf2_hmac::<sha2::Sha256>(password.as_bytes(), &salt, 100_000, &mut digest);
        }
        let mut s = self.inner.lock().unwrap();
        s.mode = mode;
        s.salt = salt;
        s.digest = digest;
        s.sessions.clear();
        s.attempts.clear();
        s.generation += 1;
        Ok(())
    }
    pub fn status(&self) -> Value {
        let s = self.inner.lock().unwrap();
        json!({"mode":match s.mode{Mode::Disabled=>"disabled",Mode::Open=>"open",Mode::Password=>"password"}})
    }
    pub fn authorized(&self, cookie: &str) -> bool {
        let s = self.inner.lock().unwrap();
        match s.mode {
            Mode::Open => true,
            Mode::Disabled => false,
            Mode::Password => s.sessions.iter().any(|(v, t)| {
                !cookie.is_empty() && v == cookie && t.elapsed() < Duration::from_secs(12 * 3600)
            }),
        }
    }
    pub fn login(&self, password: &str) -> Result<String, String> {
        let (salt, expected, generation) = {
            let mut s = self.inner.lock().unwrap();
            if s.mode != Mode::Password {
                return Err("Inicio de sesión no disponible".into());
            }
            while s
                .attempts
                .front()
                .is_some_and(|v| v.elapsed() > Duration::from_secs(60))
            {
                s.attempts.pop_front();
            }
            if s.attempts.len() >= 64 {
                return Err("Demasiados intentos; espera un minuto".into());
            }
            s.attempts.push_back(Instant::now());
            (s.salt, s.digest, s.generation)
        };
        if password.len() > 256 {
            return Err("Contraseña incorrecta".into());
        }
        let mut actual = [0; 32];
        pbkdf2::pbkdf2_hmac::<sha2::Sha256>(password.as_bytes(), &salt, 100_000, &mut actual);
        let diff = actual
            .iter()
            .zip(expected)
            .fold(0u8, |d, (a, b)| d | (a ^ b));
        if diff != 0 {
            return Err("Contraseña incorrecta".into());
        }
        let token = secret()?;
        let mut s = self.inner.lock().unwrap();
        if s.generation != generation || s.mode != Mode::Password {
            return Err("El acceso cambió; vuelve a entrar".into());
        }
        while s.sessions.len() >= 64 {
            s.sessions.pop_front();
        }
        s.sessions.push_back((token.clone(), Instant::now()));
        Ok(token)
    }
    pub fn revoke(&self) {
        let mut s = self.inner.lock().unwrap();
        s.sessions.clear();
        s.generation += 1;
    }
}
pub fn cookie_value<'a>(header: &'a str, name: &str) -> &'a str {
    header
        .split(';')
        .filter_map(|s| s.trim().split_once('='))
        .find(|(k, _)| *k == name)
        .map_or("", |(_, v)| v)
}
pub fn login_page(scope: &str) -> String {
    include_str!("../web/login.html").replace("__SCOPE__", scope)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn modes_password_revocation_and_role_isolation() {
        let a = Gate::new();
        let b = Gate::new();
        assert!(!a.authorized(""));
        a.configure("open", None).unwrap();
        assert!(a.authorized(""));
        a.configure("password", Some("prueba")).unwrap();
        assert!(a.login("incorrecta").is_err());
        let token = a.login("prueba").unwrap();
        assert!(a.authorized(&token));
        assert!(!a.authorized(""));
        b.configure("password", Some("prueba")).unwrap();
        assert!(!b.authorized(&token));
        a.revoke();
        assert!(!a.authorized(&token));
        assert_eq!(cookie_value("mm_mix=a; mm_console=b", "mm_console"), "b");
    }
}
pub fn login_http(
    gate: &Gate,
    stream: &mut std::net::TcpStream,
    scope: &str,
    body: &[u8],
) -> std::io::Result<()> {
    use std::io::Write;
    let value = serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| v["password"].as_str().map(str::to_owned));
    let result = value
        .ok_or_else(|| "Contraseña requerida".to_string())
        .and_then(|p| gate.login(&p));
    let (status, body, cookie) = match result {
        Ok(token) => (
            200,
            json!({"ok":true}).to_string(),
            format!(
                "Set-Cookie: mm_{scope}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age=43200\r\n"
            ),
        ),
        Err(e) => (403, json!({"error":e}).to_string(), String::new()),
    };
    write!(
        stream,
        "HTTP/1.1 {status} Response\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{cookie}Cache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}
