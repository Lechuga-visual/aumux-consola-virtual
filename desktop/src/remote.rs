//! Personal aux control. HTTP and the bounded command queue never run in audio callbacks.
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
}
#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Change {
    epoch: u64,
    channel: usize,
    level_db: Option<f32>,
    mute: Option<bool>,
    pre: Option<bool>,
}
struct Pending {
    key: String,
    change: Change,
    deadline: Instant,
    reply: mpsc::SyncSender<Result<Value, String>>,
}
struct Runtime {
    alive: Arc<AtomicBool>,
    task: thread::JoinHandle<()>,
}
pub struct Remote {
    state: Mutex<State>,
    queue: ArrayQueue<Pending>,
    runtime: Mutex<Option<Runtime>>,
    enabled: AtomicBool,
    port: AtomicU16,
    host: Mutex<String>,
    connections: AtomicUsize,
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
            runtime: Mutex::new(None),
            enabled: AtomicBool::new(false),
            port: AtomicU16::new(17842),
            host: Mutex::new(String::new()),
            connections: AtomicUsize::new(0),
        })
    }
    pub fn set_host(&self, host: &str) -> Result<(), String> {
        *self.host.lock().unwrap() = crate::listen::share_host(host)?;
        Ok(())
    }
    pub fn stop(&self) {
        self.enabled.store(false, Release);
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
            .map_err(|e| format!("No se pudo abrir control remoto: {e}"))?;
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
        json!({"enabled":self.enabled.load(Acquire),"port":self.port.load(Relaxed),"revision":s.revision,"host":*host,"sessions":s.sessions.iter().map(|p|json!({"id":p.id,"label":p.label,"aux":p.aux,"max_db":p.max_db,"allow_pre":p.allow_pre,"url":format!("{origin}/#key={}",p.key)})).collect::<Vec<_>>()})
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
        if s.sessions.is_empty() {
            return;
        }
        s.snapshot = json!({"epoch":s.epoch,"revision":s.revision,"running":running,"emergency":ctl.emergency.load(Relaxed),"failed":ctl.failed.load(Relaxed),"channels":c.channels.iter().enumerate().map(|(i,ch)|{let(p,r)=ctl.channel_meters[i].snapshot();json!({"index":i,"name":ch.name,"format":ch.format,"levels":ch.sends_db,"mutes":ch.send_mute,"pre":ch.pre,"channel_mute":ch.mute,"peak":p,"rms":r})}).collect::<Vec<_>>(),"buses":c.buses.iter().enumerate().map(|(i,b)|{let(p,r)=ctl.bus_meters[i].snapshot();json!({"index":i,"name":b.name,"format":b.format,"gain_db":b.gain_db,"mute":b.mute,"peak":p,"rms":r})}).collect::<Vec<_>>()});
    }
    fn view(&self, key: &str) -> Result<Value, String> {
        let s = self.state.lock().unwrap();
        let p = s
            .sessions
            .iter()
            .find(|p| p.key == key)
            .ok_or("Enlace revocado o inválido")?;
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
            json!({"epoch":snap["epoch"],"revision":snap["revision"],"running":snap["running"],"emergency":snap["emergency"],"failed":snap["failed"],"max_db":p.max_db,"allow_pre":p.allow_pre,"bus":bus,"channels":snap["channels"].as_array().unwrap().iter().map(|ch|json!({"index":ch["index"],"name":ch["name"],"format":ch["format"],"level_db":ch["levels"][p.aux-1],"mute":ch["mutes"][p.aux-1].as_bool().unwrap_or(false),"pre":ch["pre"][p.aux-1],"channel_mute":ch["channel_mute"],"peak":ch["peak"],"rms":ch["rms"]})).collect::<Vec<_>>()}),
        )
    }
    fn apply(&self, key: &str, a: &Change, c: &mut Config, ctl: &Controls) -> Result<(), String> {
        let mut s = self.state.lock().unwrap();
        let p = s
            .sessions
            .iter()
            .find(|p| p.key == key)
            .ok_or("Enlace revocado o inválido")?;
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
        let mut replies = Vec::new();
        for _ in 0..32 {
            let Some(p) = self.queue.pop() else { break };
            let result = if Instant::now() > p.deadline {
                Err("El cambio caducó; vuelve a intentarlo".into())
            } else {
                self.apply(&p.key, &p.change, c, ctl)
            };
            replies.push((p.key, p.reply, result));
        }
        if !replies.is_empty() {
            self.refresh(c, ctl, running)
        }
        for (key, reply, result) in replies {
            let _ = reply.try_send(result.and_then(|_| self.view(&key)));
        }
    }
    fn serve(&self, mut stream: TcpStream, alive: Arc<AtomicBool>) -> std::io::Result<()> {
        stream.set_read_timeout(Some(Duration::from_millis(500)))?;
        stream.set_write_timeout(Some(Duration::from_millis(500)))?;
        stream.set_nodelay(true)?;
        let until = Instant::now() + Duration::from_secs(2);
        let mut raw = Vec::new();
        let mut byte = [0];
        while raw.len() < 8192 && Instant::now() < until {
            if stream.read(&mut byte)? == 0 {
                return Ok(());
            }
            raw.push(byte[0]);
            if raw.ends_with(b"\r\n\r\n") {
                break;
            }
        }
        if !raw.ends_with(b"\r\n\r\n") {
            return send(&mut stream, 413, "text/plain", "Invalid headers");
        }
        let head = String::from_utf8_lossy(&raw);
        let mut lines = head.lines();
        let first = lines.next().unwrap_or("");
        let mut parts = first.split_whitespace();
        let method = parts.next().unwrap_or("");
        let path = parts.next().unwrap_or("");
        let headers: Vec<_> = lines
            .filter_map(|l| l.split_once(':'))
            .map(|(k, v)| (k.trim(), v.trim()))
            .collect();
        let header = |key: &str| {
            headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| *v)
        };
        if method == "GET" && path == "/" {
            return send(
                &mut stream,
                200,
                "text/html; charset=utf-8",
                include_str!("../web/remote.html"),
            );
        }
        if path != "/mix" {
            return send(&mut stream, 404, "text/plain", "Not found");
        }
        if !alive.load(Acquire) || !self.enabled.load(Acquire) {
            return send(&mut stream, 503, "text/plain", "Server stopped");
        }
        let key = header("X-Mix-Key").unwrap_or("");
        if self.view(key).is_err() {
            return send(
                &mut stream,
                403,
                "application/json",
                &json!({"error":"Enlace revocado o inválido"}).to_string(),
            );
        }
        if let Some(origin) = header("Origin") {
            if Some(origin.strip_prefix("http://").unwrap_or("")) != header("Host") {
                return send(&mut stream, 403, "text/plain", "Origin denied");
            }
        }
        if method == "GET" {
            return json_response(&mut stream, self.view(key));
        }
        if method != "POST" {
            return send(&mut stream, 405, "text/plain", "Method not allowed");
        }
        if header("Content-Type") != Some("application/json")
            || header("Transfer-Encoding").is_some()
        {
            return send(&mut stream, 400, "text/plain", "JSON required");
        }
        let Some(length) = header("Content-Length")
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|v| *v <= 2048)
        else {
            return send(&mut stream, 413, "text/plain", "Invalid body length");
        };
        let mut body = vec![0; length];
        stream.read_exact(&mut body)?;
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
                change,
                deadline: Instant::now() + Duration::from_millis(400),
                reply: tx,
            })
            .is_err()
        {
            return send(&mut stream, 503, "text/plain", "Control busy");
        }
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(result) => json_response(&mut stream, result),
            Err(_) => send(
                &mut stream,
                503,
                "application/json",
                &json!({"error":"Control ocupado; reconecta para confirmar el estado"}).to_string(),
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
                key: key.clone(),
                change: Change {
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
            serde_json::from_value::<Change>(json!({"epoch":0,"channel":0,"aux":0,"level_db":0}))
                .is_err()
        );
        r.invalidate();
        assert!(r.view(&key).is_err());
        assert!(
            r.apply(
                &key,
                &Change {
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
