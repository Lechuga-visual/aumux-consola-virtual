//! Personal aux control. HTTP and the bounded command queue never run in audio callbacks.
#[path = "remote_ws.rs"]
mod ws;
use crate::{config::Config, mixer::Controls};
use crossbeam_queue::ArrayQueue;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{
            AtomicBool, AtomicU16, AtomicUsize,
            Ordering::{Acquire, Relaxed, Release},
        },
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Clone)]
struct Session {
    id: String,
    key: String,
    label: String,
    aux: usize,
    max_db: f32,
    allow_pre: bool,
}
#[derive(Default)]
struct State {
    sessions: Vec<Session>,
    snapshot: Value,
    epoch: u64,
    revision: u64,
    shared_max: f32,
    shared_pre: bool,
}
#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Change {
    epoch: u64,
    #[serde(default)]
    aux: Option<usize>,
    channel: usize,
    level_db: Option<f32>,
    mute: Option<bool>,
    pre: Option<bool>,
}
struct Pending {
    key: String,
    cookie: Option<String>,
    change: Change,
    deadline: Instant,
    reply: mpsc::SyncSender<Result<Value, String>>,
}
struct Runtime {
    alive: Arc<AtomicBool>,
    task: thread::JoinHandle<()>,
}
pub struct Rpc {
    pub path: String,
    pub body: Option<String>,
    pub reply: mpsc::SyncSender<Result<Value, String>>,
    pub deadline: Instant,
    pub cookie: String,
}
pub struct Remote {
    state: Mutex<State>,
    queue: ArrayQueue<Pending>,
    ws_queue: ArrayQueue<ws::Pending>,
    runtime: Mutex<Option<Runtime>>,
    enabled: AtomicBool,
    port: AtomicU16,
    host: Mutex<String>,
    connections: AtomicUsize,
    pub mix_gate: crate::access::Gate,
    pub console_gate: crate::access::Gate,
    rpc: ArrayQueue<Rpc>,
    latency: Mutex<Value>,
}
fn secret() -> Result<String, String> {
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes.iter().map(|v| format!("{v:02x}")).collect())
}
impl Remote {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State::default()),
            queue: ArrayQueue::new(64),
            ws_queue: ArrayQueue::new(64),
            runtime: Mutex::new(None),
            enabled: AtomicBool::new(false),
            port: AtomicU16::new(17842),
            host: Mutex::new(String::new()),
            connections: AtomicUsize::new(0),
            mix_gate: crate::access::Gate::new(),
            console_gate: crate::access::Gate::new(),
            rpc: ArrayQueue::new(32),
            latency: Mutex::new(Value::Null),
        })
    }
    pub fn set_host(&self, host: &str) -> Result<(), String> {
        *self.host.lock().unwrap() = crate::listen::share_host(host)?;
        Ok(())
    }
    pub fn stop(&self) {
        self.enabled.store(false, Release);
        self.mix_gate.revoke();
        self.console_gate.revoke();
        self.invalidate();
        if let Some(r) = self.runtime.lock().unwrap().take() {
            r.alive.store(false, Release);
            let _ = r.task.join();
        }
    }
    pub fn changed(&self) {
        self.state.lock().unwrap().revision += 1;
    }
    pub fn invalidate(&self) {
        let mut s = self.state.lock().unwrap();
        s.sessions.clear();
        s.epoch += 1;
        s.revision += 1;
    }
    pub fn start(self: &Arc<Self>, port: u16) -> Result<Value, String> {
        self.stop();
        let host = std::env::var("MONITOR_REMOTE_BIND").unwrap_or_else(|_| "0.0.0.0".into());
        let server = TcpListener::bind((host.as_str(), port))
            .map_err(|e| crate::web_http::bind_error("control remoto", &host, port, e))?;
        server.set_nonblocking(true).map_err(|e| e.to_string())?;
        self.port.store(
            server.local_addr().map_err(|e| e.to_string())?.port(),
            Relaxed,
        );
        let alive = Arc::new(AtomicBool::new(true));
        let flag = alive.clone();
        let hub = self.clone();
        let task = thread::Builder::new()
            .name("monitor-remote".into())
            .spawn(move || {
                while flag.load(Acquire) {
                    match server.accept() {
                        Ok((stream, _)) => {
                            if hub.connections.fetch_add(1, Relaxed) >= 32 {
                                hub.connections.fetch_sub(1, Relaxed);
                                continue;
                            }
                            let h = hub.clone();
                            let a = flag.clone();
                            if thread::Builder::new()
                                .name("monitor-control-http".into())
                                .spawn(move || {
                                    let _guard = Connection(h.clone());
                                    let _ = h.serve(stream, a);
                                })
                                .is_err()
                            {
                                hub.connections.fetch_sub(1, Relaxed);
                            }
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(2))
                        }
                        Err(_) => break,
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        *self.runtime.lock().unwrap() = Some(Runtime { alive, task });
        self.enabled.store(true, Release);
        Ok(self.status())
    }
    pub fn status(&self) -> Value {
        let s = self.state.lock().unwrap();
        let host = self.host.lock().unwrap();
        let address = if host.is_empty() {
            crate::listen::local_ip()
        } else {
            host.clone()
        };
        let origin = format!(
            "http://{}:{}",
            crate::listen::url_host(&address),
            self.port.load(Relaxed)
        );
        json!({"enabled":self.enabled.load(Acquire),"port":self.port.load(Relaxed),"revision":s.revision,"host":*host,"mix_access":self.mix_gate.status(),"shared_max_db":s.shared_max,"shared_allow_pre":s.shared_pre,"console_access":self.console_gate.status(),"generic_url":format!("{origin}/"),"console_url":format!("{origin}/console/"),"sessions":s.sessions.iter().map(|p|json!({"id":p.id,"label":p.label,"aux":p.aux,"max_db":p.max_db,"allow_pre":p.allow_pre,"url":format!("{origin}/#key={}",p.key)})).collect::<Vec<_>>()})
    }
    pub fn admin(self: &Arc<Self>, v: &Value, c: &Config) -> Result<Value, String> {
        match v["action"].as_str().unwrap_or("") {
            "start" => {
                let port = v["port"].as_u64().unwrap_or(17842);
                if port > 65535 {
                    return Err("Puerto inválido".into());
                }
                return self.start(port as u16);
            }
            "stop" => self.stop(),
            "create" => {
                if !self.enabled.load(Acquire) {
                    return Err("Inicia el servidor de control".into());
                }
                let aux = v["aux"].as_u64().ok_or("Auxiliar requerido")? as usize;
                if aux == 0 || aux >= c.buses.len() {
                    return Err("Solo se puede asignar un auxiliar existente".into());
                }
                let label = v["label"].as_str().unwrap_or("").trim();
                if label.is_empty() || label.len() > 80 {
                    return Err("Nombre requerido (máximo 80 bytes)".into());
                }
                let max_db = v["max_db"].as_f64().unwrap_or(0.0) as f32;
                if !max_db.is_finite() || !(-60.0..=6.0).contains(&max_db) {
                    return Err("Límite inválido".into());
                }
                let mut s = self.state.lock().unwrap();
                if s.sessions.len() >= 32 {
                    return Err("Máximo 32 enlaces".into());
                }
                s.sessions.push(Session {
                    id: secret()?,
                    key: secret()?,
                    label: label.into(),
                    aux,
                    max_db,
                    allow_pre: v["allow_pre"].as_bool().unwrap_or(false),
                });
            }
            "revoke" => {
                let id = v["id"].as_str().ok_or("Enlace requerido")?;
                self.state.lock().unwrap().sessions.retain(|s| s.id != id);
            }
            "access" => {
                let mode = v["mode"].as_str().ok_or("Modo requerido")?;
                match v["scope"].as_str().unwrap_or("") {
                    "mix" => {
                        let max = v["max_db"].as_f64().unwrap_or(0.0) as f32;
                        if !max.is_finite() || !(-60.0..=6.0).contains(&max) {
                            return Err("Límite inválido".into());
                        }
                        self.mix_gate.configure(mode, v["password"].as_str())?;
                        let mut s = self.state.lock().unwrap();
                        s.shared_max = max;
                        s.shared_pre = v["allow_pre"].as_bool().unwrap_or(false);
                        s.revision += 1;
                    }
                    "console" => self.console_gate.configure(mode, v["password"].as_str())?,
                    _ => return Err("Ámbito inválido".into()),
                }
            }
            "revoke_all" => self.invalidate(),
            _ => return Err("Acción desconocida".into()),
        }
        Ok(self.status())
    }
    pub fn refresh(&self, c: &Config, ctl: &Controls, running: bool) {
        if !self.enabled.load(Acquire) {
            return;
        }
        let mut s = self.state.lock().unwrap();
        if s.sessions.is_empty() && self.mix_gate.status()["mode"] == "disabled" {
            return;
        }
        s.snapshot = json!({"epoch":s.epoch,"revision":s.revision,"running":running,"emergency":ctl.emergency.load(Relaxed),"failed":ctl.failed.load(Relaxed),"channels":c.channels.iter().enumerate().map(|(i,ch)|{let(p,r)=ctl.channel_meters[i].snapshot();json!({"index":i,"name":ch.name,"format":ch.format,"levels":ch.sends_db,"mutes":ch.send_mute,"pre":ch.pre,"channel_mute":ch.mute,"peak":p,"rms":r})}).collect::<Vec<_>>(),"buses":c.buses.iter().enumerate().map(|(i,b)|{let(p,r)=ctl.bus_meters[i].snapshot();json!({"index":i,"name":b.name,"format":b.format,"gain_db":b.gain_db,"mute":b.mute,"peak":p,"rms":r})}).collect::<Vec<_>>()});
    }
    fn resolve(
        &self,
        s: &State,
        key: &str,
        aux: Option<usize>,
        cookie: Option<&str>,
    ) -> Result<Session, String> {
        if !key.is_empty() {
            let p = s
                .sessions
                .iter()
                .find(|p| p.key == key)
                .ok_or("Enlace revocado o inválido")?;
            if aux.is_some_and(|a| a != p.aux) {
                return Err("Enlace limitado a su auxiliar".into());
            }
            return Ok(p.clone());
        }
        if !self.mix_gate.authorized(cookie.unwrap_or("")) {
            return Err("Autenticación requerida".into());
        }
        let aux = aux.unwrap_or(1);
        if aux == 0 || aux >= s.snapshot["buses"].as_array().map_or(0, Vec::len) {
            return Err("Selecciona un auxiliar existente".into());
        }
        Ok(Session {
            id: String::new(),
            key: String::new(),
            label: "Mi mezcla".into(),
            aux,
            max_db: s.shared_max,
            allow_pre: s.shared_pre,
        })
    }
    #[cfg(test)]
    fn view(&self, key: &str) -> Result<Value, String> {
        self.view_for(key, None, None)
    }
    fn view_for(
        &self,
        key: &str,
        aux: Option<usize>,
        cookie: Option<&str>,
    ) -> Result<Value, String> {
        let s = self.state.lock().unwrap();
        let p = self.resolve(&s, key, aux, cookie)?;
        if !self.enabled.load(Acquire) {
            return Err("Servidor detenido".into());
        }
        let snap = &s.snapshot;
        if !snap["channels"].is_array() {
            return Err("Sincronizando".into());
        }
        let mut bus = snap["buses"][p.aux].clone();
        bus["label"] = json!(p.label);
        Ok(
            json!({"epoch":snap["epoch"],"revision":snap["revision"],"running":snap["running"],"emergency":snap["emergency"],"failed":snap["failed"],"max_db":p.max_db,"allow_pre":p.allow_pre,"shared":key.is_empty(),"choices":if key.is_empty(){snap["buses"].as_array().unwrap().iter().skip(1).map(|b|json!({"index":b["index"],"name":b["name"],"format":b["format"]})).collect::<Vec<_>>()}else{vec![]},"latency":*self.latency.lock().unwrap(),"bus":bus,"channels":snap["channels"].as_array().unwrap().iter().map(|ch|json!({"index":ch["index"],"name":ch["name"],"format":ch["format"],"level_db":ch["levels"][p.aux-1],"mute":ch["mutes"][p.aux-1].as_bool().unwrap_or(false),"pre":ch["pre"][p.aux-1],"channel_mute":ch["channel_mute"],"peak":ch["peak"],"rms":ch["rms"]})).collect::<Vec<_>>()}),
        )
    }
    #[cfg(test)]
    fn apply(&self, key: &str, a: &Change, c: &mut Config, ctl: &Controls) -> Result<(), String> {
        self.apply_for(key, a, c, ctl, None)
    }
    fn apply_for(
        &self,
        key: &str,
        a: &Change,
        c: &mut Config,
        ctl: &Controls,
        cookie: Option<&str>,
    ) -> Result<(), String> {
        let mut s = self.state.lock().unwrap();
        let p = self.resolve(&s, key, a.aux, cookie)?;
        if !self.enabled.load(Acquire) || a.epoch != s.epoch {
            return Err("La mezcla cambió; vuelve a conectar".into());
        }
        if p.aux == 0 || p.aux >= c.buses.len() || a.channel >= c.channels.len() {
            return Err("Canal o auxiliar inexistente".into());
        }
        if a.level_db.is_none() && a.mute.is_none() && a.pre.is_none() {
            return Err("Cambio vacío".into());
        }
        if a.level_db
            .is_some_and(|v| !v.is_finite() || !(-90.0..=p.max_db).contains(&v))
        {
            return Err("Nivel fuera del límite asignado".into());
        }
        if a.pre.is_some() && !p.allow_pre {
            return Err("PRE/POST reservado al operador".into());
        }
        let aux = p.aux - 1;
        let ch = &mut c.channels[a.channel];
        ch.send_mute.resize(c.buses.len() - 1, false);
        if let Some(v) = a.level_db {
            ch.sends_db[aux] = v
        }
        if let Some(v) = a.mute {
            ch.send_mute[aux] = v
        }
        if let Some(v) = a.pre {
            ch.pre[aux] = v
        }
        ctl.set_send(
            a.channel,
            aux,
            ch.sends_db[aux],
            ch.pre[aux],
            ch.send_mute[aux],
        );
        s.revision += 1;
        Ok(())
    }
    pub fn drain(&self, c: &mut Config, ctl: &Controls, running: bool) {
        ws::drain(self, c, ctl, running);
        let mut replies = Vec::new();
        for _ in 0..32 {
            let Some(p) = self.queue.pop() else { break };
            let result = if Instant::now() > p.deadline {
                Err("El cambio caducó; vuelve a intentarlo".into())
            } else {
                self.apply_for(&p.key, &p.change, c, ctl, p.cookie.as_deref())
            };
            replies.push((p.key, p.change.aux, p.cookie, p.reply, result));
        }
        if !replies.is_empty() {
            self.refresh(c, ctl, running)
        }
        for (key, aux, cookie, reply, result) in replies {
            let _ =
                reply.try_send(result.and_then(|_| self.view_for(&key, aux, cookie.as_deref())));
        }
    }
    pub fn set_latency(&self, value: Value) {
        *self.latency.lock().unwrap() = value;
    }
    pub fn take_rpc(&self) -> Option<Rpc> {
        self.rpc.pop()
    }
    fn serve(&self, mut stream: TcpStream, alive: Arc<AtomicBool>) -> std::io::Result<()> {
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
        stream.set_nodelay(true)?;
        let raw = match crate::web_http::headers(&mut stream) {
            Ok(v) => v,
            Err(e) => return crate::web_http::request_error(&mut stream, e),
        };
        let head = String::from_utf8_lossy(&raw);
        let mut lines = head.lines();
        let mut first = lines.next().unwrap_or("").split_whitespace();
        let method = first.next().unwrap_or("");
        let uri = first.next().unwrap_or("");
        let (path, query) = uri.split_once('?').unwrap_or((uri, ""));
        let headers: Vec<_> = lines
            .filter_map(|v| v.split_once(':'))
            .map(|(k, v)| (k.trim(), v.trim()))
            .collect();
        let header = |name: &str| {
            headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| *v)
        };
        if !alive.load(Acquire) || !self.enabled.load(Acquire) {
            return send(&mut stream, 503, "text/plain", "Server stopped");
        }
        if let Some(origin) = header("Origin") {
            if Some(origin.strip_prefix("http://").unwrap_or("")) != header("Host") {
                return send(&mut stream, 403, "text/plain", "Origin denied");
            }
        }
        if method == "GET" && path == "/brand-logo.png" {
            let logo = include_bytes!("../assets/aumux-logo.png");
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\nX-Content-Type-Options: nosniff\r\n\r\n",
                logo.len()
            )?;
            return stream.write_all(logo);
        }
        if path == "/mix/ws" {
            return ws::serve(self, stream, raw, alive);
        }
        let cookies = header("Cookie").unwrap_or("");
        let mix_cookie = crate::access::cookie_value(cookies, "mm_mix");
        let console_cookie = crate::access::cookie_value(cookies, "mm_console");
        if method == "POST" && (path == "/login/mix" || path == "/login/console") {
            let body = read_body(
                &mut stream,
                header("Content-Length"),
                header("Content-Type"),
                header("Transfer-Encoding"),
                2048,
            )?;
            return crate::access::login_http(
                if path.ends_with("mix") {
                    &self.mix_gate
                } else {
                    &self.console_gate
                },
                &mut stream,
                if path.ends_with("mix") {
                    "mix"
                } else {
                    "console"
                },
                &body,
            );
        }
        if method == "GET" && path == "/access" {
            return send(
                &mut stream,
                200,
                "application/json",
                &json!({"mix":self.mix_gate.status(),"console":self.console_gate.status()})
                    .to_string(),
            );
        }
        if method == "GET" && path == "/" {
            return send(
                &mut stream,
                200,
                "text/html; charset=utf-8",
                include_str!("../web/remote.html"),
            );
        }
        if path == "/console/" || path == "/console" {
            if self.console_gate.status()["mode"] == "disabled" {
                return send(
                    &mut stream,
                    403,
                    "text/plain",
                    "Consola remota deshabilitada",
                );
            }
            if !self.console_gate.authorized(console_cookie) {
                return send(
                    &mut stream,
                    200,
                    "text/html; charset=utf-8",
                    &crate::access::login_page("console"),
                );
            }
            return send(
                &mut stream,
                200,
                "text/html; charset=utf-8",
                &include_str!("../web/index.html").replace("__TOKEN__", "remote-console"),
            );
        }
        if matches!(
            path,
            "/console.css"
                | "/console.js"
                | "/workspace-ui.js"
                | "/effects-ui.js"
                | "/plugin-browser.js"
                | "/tools-ui.js"
                | "/panels-ui.js"
                | "/diagnostics-ui.js"
        ) {
            if !self.console_gate.authorized(console_cookie) {
                return send(&mut stream, 403, "text/plain", "Consola protegida");
            }
            let (kind, body) = match path {
                "/console.css" => ("text/css", include_str!("../web/console.css")),
                "/console.js" => ("text/javascript", include_str!("../web/console.js")),
                "/workspace-ui.js" => ("text/javascript", include_str!("../web/workspace-ui.js")),
                "/panels-ui.js" => ("text/javascript", include_str!("../web/panels-ui.js")),
                "/diagnostics-ui.js" => {
                    ("text/javascript", include_str!("../web/diagnostics-ui.js"))
                }
                "/tools-ui.js" => ("text/javascript", include_str!("../web/tools-ui.js")),
                "/plugin-browser.js" => {
                    ("text/javascript", include_str!("../web/plugin-browser.js"))
                }
                _ => ("text/javascript", include_str!("../web/effects-ui.js")),
            };
            return send(&mut stream, 200, kind, body);
        }
        if path.starts_with("/api/") {
            if !self.console_gate.authorized(console_cookie) {
                return send(
                    &mut stream,
                    403,
                    "application/json",
                    "{\"error\":\"Consola protegida\"}",
                );
            }
            let body = if method == "POST" {
                Some(
                    String::from_utf8(read_body(
                        &mut stream,
                        header("Content-Length"),
                        header("Content-Type"),
                        header("Transfer-Encoding"),
                        2097152,
                    )?)
                    .map_err(|_| std::io::Error::other("Invalid UTF-8"))?,
                )
            } else if method == "GET" {
                None
            } else {
                return send(&mut stream, 405, "text/plain", "Method not allowed");
            };
            let (tx, rx) = mpsc::sync_channel(1);
            if self
                .rpc
                .push(Rpc {
                    path: path.into(),
                    body,
                    reply: tx,
                    deadline: Instant::now() + Duration::from_millis(1500),
                    cookie: console_cookie.into(),
                })
                .is_err()
            {
                return send(&mut stream, 503, "text/plain", "Control busy");
            }
            return match rx.recv_timeout(Duration::from_secs(2)) {
                Ok(v) => json_response(&mut stream, v),
                Err(_) => send(
                    &mut stream,
                    503,
                    "application/json",
                    "{\"error\":\"Consola ocupada\"}",
                ),
            };
        }
        if path != "/mix" {
            return send(&mut stream, 404, "text/plain", "Not found");
        }
        let key = header("X-Mix-Key").unwrap_or("");
        let aux = query
            .split('&')
            .filter_map(|v| v.split_once('='))
            .find(|(k, _)| *k == "aux")
            .and_then(|(_, v)| v.parse().ok());
        let view = self.view_for(key, aux, Some(mix_cookie));
        if view.is_err() {
            return send(
                &mut stream,
                403,
                "application/json",
                &json!({"error":view.unwrap_err(),"access":self.mix_gate.status()}).to_string(),
            );
        }
        if method == "GET" {
            return json_response(&mut stream, view);
        }
        if method != "POST" {
            return send(&mut stream, 405, "text/plain", "Method not allowed");
        }
        let body = read_body(
            &mut stream,
            header("Content-Length"),
            header("Content-Type"),
            header("Transfer-Encoding"),
            2048,
        )?;
        let change: Change = match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(e) => {
                return send(
                    &mut stream,
                    400,
                    "application/json",
                    &json!({"error":e.to_string()}).to_string(),
                );
            }
        };
        let (tx, rx) = mpsc::sync_channel(1);
        if self
            .queue
            .push(Pending {
                key: key.into(),
                cookie: Some(mix_cookie.into()),
                change,
                deadline: Instant::now() + Duration::from_millis(400),
                reply: tx,
            })
            .is_err()
        {
            return send(&mut stream, 503, "text/plain", "Control busy");
        }
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(v) => json_response(&mut stream, v),
            Err(_) => send(
                &mut stream,
                503,
                "application/json",
                "{\"error\":\"Control ocupado; reconecta\"}",
            ),
        }
    }
}
fn json_response(s: &mut TcpStream, result: Result<Value, String>) -> std::io::Result<()> {
    match result {
        Ok(v) => send(s, 200, "application/json", &v.to_string()),
        Err(e) => send(s, 400, "application/json", &json!({"error":e}).to_string()),
    }
}
fn send(s: &mut TcpStream, status: u16, kind: &str, body: &str) -> std::io::Result<()> {
    write!(
        s,
        "HTTP/1.1 {status} Response\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nX-Content-Type-Options: nosniff\r\n\r\n",
        body.len()
    )?;
    s.write_all(body.as_bytes())
}
struct Connection(Arc<Remote>);
impl Drop for Connection {
    fn drop(&mut self) {
        self.0.connections.fetch_sub(1, Relaxed);
    }
}

/// Preserve concurrent remote sends when the desktop edits unrelated configuration.
#[cfg(test)]
pub fn merge_sends(candidate: &mut Config, base: &Config, current: &Config) {
    if candidate.channels.len() != base.channels.len()
        || candidate.buses.len() != base.buses.len()
        || current.channels.len() != base.channels.len()
        || current.buses.len() != base.buses.len()
    {
        return;
    }
    for (i, ch) in candidate.channels.iter_mut().enumerate() {
        let (Some(old), Some(now)) = (base.channels.get(i), current.channels.get(i)) else {
            continue;
        };
        for a in 0..ch.sends_db.len() {
            if ch.sends_db.get(a) == old.sends_db.get(a) {
                if let Some(v) = now.sends_db.get(a) {
                    ch.sends_db[a] = *v
                }
            }
            if ch.pre.get(a) == old.pre.get(a) {
                if let Some(v) = now.pre.get(a) {
                    ch.pre[a] = *v
                }
            }
            if ch.send_mute.get(a) == old.send_mute.get(a) {
                if let Some(v) = now.send_mute.get(a) {
                    ch.send_mute[a] = *v
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mixer::Mixer;
    fn setup() -> (Arc<Remote>, Config, Arc<Controls>, String) {
        let mut c = Config::default();
        c.normalise();
        for ch in &mut c.channels {
            ch.mute = true;
        }
        c.channels[0].mute = false;
        c.channels[0].sends_db[0] = 0.0;
        for b in &mut c.buses {
            b.gain_db = 0.0;
        }
        let ctl = Controls::new(&c);
        let r = Remote::new();
        r.enabled.store(true, Release);
        r.admin(
            &json!({"action":"create","aux":1,"label":"Persona","max_db":0}),
            &c,
        )
        .unwrap();
        r.refresh(&c, &ctl, true);
        let key = r.state.lock().unwrap().sessions[0].key.clone();
        (r, c, ctl, key)
    }
    #[test]
    fn personal_permissions_and_mute_only_change_assigned_audio_send() {
        let (r, mut c, ctl, key) = setup();
        let mut a = Change {
            aux: None,
            epoch: 0,
            channel: 0,
            level_db: Some(-6.0206),
            mute: None,
            pre: None,
        };
        r.apply(&key, &a, &mut c, &ctl).unwrap();
        assert_eq!(c.channels[0].gain_db, 0.0);
        assert_eq!(c.channels[0].sends_db[1], -90.0);
        assert!(r.apply("wrong", &a, &mut c, &ctl).is_err());
        a.level_db = Some(1.0);
        assert!(r.apply(&key, &a, &mut c, &ctl).is_err());
        a.level_db = None;
        a.pre = Some(false);
        assert!(r.apply(&key, &a, &mut c, &ctl).is_err());
        a.pre = None;
        let mut m = Mixer::new(&c, ctl.clone()).unwrap();
        let mut input = vec![0.0; 256 * 8];
        for f in 0..256 {
            input[f * 8] = 0.2;
        }
        let mut out = vec![0.0; 256 * 2];
        for _ in 0..3 {
            crate::test_support::no_heap(|| m.process(&input, &mut out, 256, 2));
        }
        assert!((m.buses[0][0][255] - 0.2).abs() < 1e-5);
        assert!((m.buses[1][0][255] - 0.1).abs() < 1e-5);
        a.mute = Some(true);
        r.apply(&key, &a, &mut c, &ctl).unwrap();
        for _ in 0..2 {
            crate::test_support::no_heap(|| m.process(&input, &mut out, 256, 2));
        }
        assert_eq!(m.buses[1][0][255], 0.0);
        assert!((m.buses[0][0][255] - 0.2).abs() < 1e-5);
        assert_eq!(c.channels[0].sends_db[0], -6.0206);
        a.mute = Some(false);
        r.apply(&key, &a, &mut c, &ctl).unwrap();
        for _ in 0..2 {
            crate::test_support::no_heap(|| m.process(&input, &mut out, 256, 2));
        }
        assert!((m.buses[1][0][255] - 0.1).abs() < 1e-5);
        let (p, _) = ctl.channel_meters[0].snapshot();
        assert!(p[0] > 0.19);
        let (p, _) = ctl.channel_meters[0].read();
        assert!(p[0] > 0.19);
        input.fill(0.0);
        m.process(&input, &mut out, 256, 2);
        assert_eq!(ctl.channel_meters[0].snapshot().0, [0.0; 2]);
    }
    #[test]
    fn desktop_three_way_send_merge_keeps_disjoint_remote_changes() {
        let mut base = Config::default();
        base.normalise();
        let mut current = base.clone();
        current.channels[0].sends_db[0] = -12.0;
        current.channels[0].send_mute[0] = true;
        current.channels[0].pre[0] = false;
        let mut candidate = base.clone();
        candidate.channels[0].gain_db = -6.0;
        candidate.channels[1].sends_db[0] = -24.0;
        merge_sends(&mut candidate, &base, &current);
        assert_eq!(candidate.channels[0].sends_db[0], -12.0);
        assert!(candidate.channels[0].send_mute[0]);
        assert!(!candidate.channels[0].pre[0]);
        assert_eq!(candidate.channels[0].gain_db, -6.0);
        assert_eq!(candidate.channels[1].sends_db[0], -24.0);
        candidate.channels[0].sends_db[0] = -3.0;
        merge_sends(&mut candidate, &base, &current);
        assert_eq!(candidate.channels[0].sends_db[0], -3.0);
    }
    #[test]
    fn revoked_links_expired_commands_and_unknown_fields_never_apply() {
        let (r, mut c, ctl, key) = setup();
        let (tx, rx) = mpsc::sync_channel(1);
        r.queue
            .push(Pending {
                cookie: None,
                key: key.clone(),
                change: Change {
                    aux: None,
                    epoch: 0,
                    channel: 0,
                    level_db: Some(-9.0),
                    mute: None,
                    pre: None,
                },
                deadline: Instant::now() - Duration::from_millis(1),
                reply: tx,
            })
            .ok()
            .unwrap();
        r.drain(&mut c, &ctl, true);
        assert!(rx.recv().unwrap().is_err());
        assert_eq!(c.channels[0].sends_db[0], 0.0);
        assert!(
            serde_json::from_value::<Change>(json!({"epoch":0,"channel":0,"bus":0,"level_db":0}))
                .is_err()
        );
        r.invalidate();
        assert!(r.view(&key).is_err());
        assert!(
            r.apply(
                &key,
                &Change {
                    aux: None,
                    epoch: 0,
                    channel: 0,
                    level_db: Some(-9.0),
                    mute: None,
                    pre: None
                },
                &mut c,
                &ctl
            )
            .is_err()
        );
        assert_eq!(c.channels[0].sends_db[0], 0.0);
    }
    #[test]
    fn tailscale_addresses_are_supported_without_accepting_urls_or_paths() {
        for host in [
            "100.101.102.103",
            "studio.tailnet-name.ts.net",
            "studio-pc",
            "fd7a:115c:a1e0::1234",
        ] {
            assert!(crate::listen::share_host(host).is_ok());
        }
        assert_eq!(
            crate::listen::url_host("fd7a:115c:a1e0::1234"),
            "[fd7a:115c:a1e0::1234]"
        );
        for host in [
            "http://studio",
            "studio:17842",
            "a/path",
            "x?key=abc",
            "a b",
            "-bad.ts.net",
        ] {
            assert!(crate::listen::share_host(host).is_err());
        }
    }
}

#[cfg(test)]
mod snapshot_version_tests {
    use super::*;
    #[test]
    fn view_version_always_belongs_to_the_published_values() {
        let mut c = Config::default();
        c.normalise();
        let ctl = Controls::new(&c);
        let r = Remote::new();
        r.enabled.store(true, Release);
        r.admin(&json!({"action":"create","label":"Prueba","aux":1}), &c)
            .unwrap();
        r.refresh(&c, &ctl, true);
        let key = r.state.lock().unwrap().sessions[0].key.clone();
        let before = r.view(&key).unwrap();
        r.apply(
            &key,
            &Change {
                aux: None,
                epoch: before["epoch"].as_u64().unwrap(),
                channel: 0,
                level_db: Some(-12.0),
                mute: None,
                pre: None,
            },
            &mut c,
            &ctl,
        )
        .unwrap();
        let unpublished = r.view(&key).unwrap();
        assert_eq!(unpublished["revision"], before["revision"]);
        assert_eq!(
            unpublished["channels"][0]["level_db"],
            before["channels"][0]["level_db"]
        );
        r.refresh(&c, &ctl, true);
        let after = r.view(&key).unwrap();
        assert!(after["revision"].as_u64().unwrap() > before["revision"].as_u64().unwrap());
        assert_eq!(after["channels"][0]["level_db"], -12.0);
    }
}

fn read_body(
    s: &mut TcpStream,
    length: Option<&str>,
    kind: Option<&str>,
    transfer: Option<&str>,
    max: usize,
) -> std::io::Result<Vec<u8>> {
    let result = (|| {
        if kind != Some("application/json") || transfer.is_some() {
            return Err(std::io::Error::other("JSON required"));
        }
        let n = length
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|v| *v <= max)
            .ok_or_else(|| std::io::Error::other("Invalid body length"))?;
        let mut body = vec![0; n];
        s.read_exact(&mut body)?;
        Ok(body)
    })();
    if let Err(ref error) = result {
        crate::web_http::request_error(s, std::io::Error::new(error.kind(), error.to_string()))?;
    }
    result
}

/// Three-way merge for simultaneous operator consoles: untouched fields follow the current server.
pub fn merge_values(base: &Value, candidate: &Value, current: &Value) -> Value {
    if candidate == base {
        return current.clone();
    }
    // Insert instances are atomic when their identity changes: stale parameter
    // edits must never migrate into a replacement, even of the same plugin class.
    if base["id"].is_string() && base["path"].is_string() && base["instance"].is_string() {
        if candidate["instance"] != base["instance"] {
            return candidate.clone();
        }
        if current["instance"] != base["instance"] {
            return current.clone();
        }
    }
    match (base, candidate, current) {
        (Value::Object(b), Value::Object(n), Value::Object(c)) => Value::Object(
            n.iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        match (b.get(k), c.get(k)) {
                            (Some(old), Some(now)) => merge_values(old, v, now),
                            _ => v.clone(),
                        },
                    )
                })
                .collect(),
        ),
        (Value::Array(b), Value::Array(n), Value::Array(c))
            if b.len() == n.len() && b.len() == c.len() =>
        {
            Value::Array(
                b.iter()
                    .zip(n)
                    .zip(c)
                    .map(|((old, next), now)| merge_values(old, next, now))
                    .collect(),
            )
        }
        _ => candidate.clone(),
    }
}
#[cfg(test)]
mod merge_operator_tests {
    use super::*;
    #[test]
    fn stale_plugin_parameters_never_migrate_into_a_replacement_instance() {
        let base = json!({"path":"native:delay","id":"native/mda/delay","instance":"original","params":{"time":1}});
        let mut stale = base.clone();
        stale["params"]["time"] = json!(2);
        let mut replaced = base.clone();
        replaced["instance"] = json!("replacement");
        replaced["params"]["time"] = json!(3);
        assert_eq!(merge_values(&base, &stale, &replaced), replaced);
        assert_eq!(merge_values(&base, &replaced, &stale), replaced);
    }

    #[test]
    fn simultaneous_operators_preserve_untouched_names_levels_and_parameters() {
        let base =
            json!({"channels":[{"name":"Voz","gain_db":0,"pan":0,"eq":{"q":1}}],"master":-12});
        let remote = json!({"channels":[{"name":"Voz nueva","gain_db":-6,"pan":0,"eq":{"q":2}}],"master":-12});
        let local =
            json!({"channels":[{"name":"Voz","gain_db":0,"pan":0.4,"eq":{"q":1}}],"master":-3});
        let merged = merge_values(&base, &local, &remote);
        assert_eq!(
            merged["channels"][0],
            json!({"name":"Voz nueva","gain_db":-6,"pan":0.4,"eq":{"q":2}})
        );
        assert_eq!(merged["master"], -3);
    }
}
