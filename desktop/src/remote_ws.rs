//! Persistent personal-mix control. All socket work stays off the audio thread.
use super::*;
use std::{collections::VecDeque, io::Cursor};
use tungstenite::{
    Error, Message, WebSocket,
    handshake::server::{Request, Response},
    protocol::WebSocketConfig,
};

const MAX_BYTES: usize = 64 * 1024;
const HEARTBEAT: Duration = Duration::from_secs(1);
const DEAD_CONNECTION: Duration = Duration::from_secs(6);

struct PrefixStream {
    prefix: Cursor<Vec<u8>>,
    socket: TcpStream,
}
impl Read for PrefixStream {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        if self.prefix.position() < self.prefix.get_ref().len() as u64 {
            self.prefix.read(bytes)
        } else {
            self.socket.read(bytes)
        }
    }
}
impl Write for PrefixStream {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.socket.write(bytes)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.socket.flush()
    }
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
enum Incoming {
    Hello {
        #[serde(default)]
        key: String,
        aux: Option<usize>,
    },
    Change {
        seq: u64,
        epoch: u64,
        aux: Option<usize>,
        changes: Vec<Delta>,
    },
    Ping {
        #[serde(default)]
        sent: Value,
    },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Delta {
    channel: usize,
    level_db: Option<f32>,
    mute: Option<bool>,
    pre: Option<bool>,
}
pub(super) struct Pending {
    key: String,
    cookie: String,
    aux: usize,
    epoch: u64,
    changes: Vec<Delta>,
    deadline: Instant,
    live: Arc<AtomicBool>,
    reply: mpsc::SyncSender<Result<Value, Fault>>,
}
struct Lease(Arc<AtomicBool>);
impl Drop for Lease {
    fn drop(&mut self) {
        self.0.store(false, Release);
    }
}
#[derive(Debug)]
struct Fault {
    code: &'static str,
    message: String,
}
impl Fault {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
    fn wire(&self, seq: Option<u64>, remote: &Remote) -> Value {
        let mut v = json!({"type":"error","code":self.code,"message":self.message});
        if let Some(seq) = seq {
            v["seq"] = json!(seq);
        }
        if self.code == "auth" {
            v["access"] = remote.mix_gate.status();
        }
        v
    }
}
fn view_error(message: String) -> Fault {
    let code = if message == "Sincronizando" {
        "busy"
    } else if message == "Selecciona un auxiliar existente" {
        "invalid"
    } else {
        "auth"
    };
    Fault::new(code, message)
}

/// Validate the complete batch before changing any channel. Authorization and
/// lease/deadline checks happen here, immediately before applying, not only at
/// reception. A disconnected socket can never replay its queued changes later.
fn apply(remote: &Remote, p: &Pending, c: &mut Config, ctl: &Controls) -> Result<(), Fault> {
    let mut state = remote.state.lock().unwrap();
    if !remote.enabled.load(Acquire) || !p.live.load(Acquire) {
        return Err(Fault::new(
            "busy",
            "La conexión se cerró; sincroniza de nuevo",
        ));
    }
    if p.epoch != state.epoch {
        return Err(Fault::new(
            "topology",
            "La mezcla cambió; vuelve a conectar",
        ));
    }
    let session = remote
        .resolve(&state, &p.key, Some(p.aux), Some(&p.cookie))
        .map_err(view_error)?;
    if Instant::now() > p.deadline {
        return Err(Fault::new("busy", "El cambio caducó; no se repetirá"));
    }
    if p.aux == 0 || p.aux >= c.buses.len() {
        return Err(Fault::new("topology", "El auxiliar ya no existe"));
    }
    for change in &p.changes {
        if change.channel >= c.channels.len() {
            return Err(Fault::new("invalid", "Canal inexistente"));
        }
        if change.level_db.is_none() && change.mute.is_none() && change.pre.is_none() {
            return Err(Fault::new("invalid", "Cambio vacío"));
        }
        if change
            .level_db
            .is_some_and(|v| !v.is_finite() || !(-90.0..=session.max_db).contains(&v))
        {
            return Err(Fault::new("invalid", "Nivel fuera del límite asignado"));
        }
        if change.pre.is_some() && !session.allow_pre {
            return Err(Fault::new("invalid", "PRE/POST reservado al operador"));
        }
    }
    if !p.live.load(Acquire) {
        return Err(Fault::new("busy", "La conexión se cerró"));
    }
    let aux = p.aux - 1;
    for change in &p.changes {
        let ch = &mut c.channels[change.channel];
        ch.send_mute.resize(c.buses.len() - 1, false);
        if let Some(v) = change.level_db {
            ch.sends_db[aux] = v;
        }
        if let Some(v) = change.mute {
            ch.send_mute[aux] = v;
        }
        if let Some(v) = change.pre {
            ch.pre[aux] = v;
        }
        ctl.set_send(
            change.channel,
            aux,
            ch.sends_db[aux],
            ch.pre[aux],
            ch.send_mute[aux],
        );
    }
    state.revision += 1;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Arc<Remote>, Config, Arc<Controls>, String) {
        let remote = Remote::new();
        remote.enabled.store(true, Release);
        let mut c = Config::default();
        c.normalise();
        let ctl = Controls::new(&c);
        let key = "personal-test-key".to_owned();
        remote.state.lock().unwrap().sessions.push(Session {
            id: "test".into(),
            key: key.clone(),
            label: "Test".into(),
            aux: 1,
            max_db: 0.0,
            allow_pre: false,
        });
        remote.refresh(&c, &ctl, true);
        (remote, c, ctl, key)
    }
    fn pending(key: String, live: bool, changes: Vec<Delta>) -> Pending {
        let (reply, _) = mpsc::sync_channel(1);
        Pending {
            key,
            cookie: String::new(),
            aux: 1,
            epoch: 0,
            changes,
            deadline: Instant::now() + Duration::from_secs(1),
            live: Arc::new(AtomicBool::new(live)),
            reply,
        }
    }
    fn level(channel: usize, value: f32) -> Delta {
        Delta {
            channel,
            level_db: Some(value),
            mute: None,
            pre: None,
        }
    }
    #[test]
    fn complete_batch_is_validated_before_applying() {
        let (remote, mut c, ctl, key) = fixture();
        let original = c.channels[0].sends_db[0];
        let p = pending(key, true, vec![level(0, -12.0), level(1, 6.0)]);
        assert_eq!(
            apply(&remote, &p, &mut c, &ctl).unwrap_err().code,
            "invalid"
        );
        assert_eq!(c.channels[0].sends_db[0], original);
        assert_eq!(remote.state.lock().unwrap().revision, 0);
    }
    #[test]
    fn disconnected_expired_and_revoked_batches_never_apply() {
        let (remote, mut c, ctl, key) = fixture();
        let original = c.channels[0].sends_db[0];
        let mut p = pending(key, false, vec![level(0, -12.0)]);
        assert_eq!(apply(&remote, &p, &mut c, &ctl).unwrap_err().code, "busy");
        p.live.store(true, Release);
        p.deadline = Instant::now() - Duration::from_millis(1);
        assert_eq!(apply(&remote, &p, &mut c, &ctl).unwrap_err().code, "busy");
        p.deadline = Instant::now() + Duration::from_secs(1);
        remote.state.lock().unwrap().sessions.clear();
        assert_eq!(apply(&remote, &p, &mut c, &ctl).unwrap_err().code, "auth");
        assert_eq!(c.channels[0].sends_db[0], original);
    }
    #[test]
    fn valid_batch_updates_only_assigned_aux_and_revises_once() {
        let (remote, mut c, ctl, key) = fixture();
        let other = c.channels[0].sends_db[1];
        let p = pending(key, true, vec![level(0, -12.0), level(1, -6.0)]);
        apply(&remote, &p, &mut c, &ctl).unwrap();
        assert_eq!(c.channels[0].sends_db[0], -12.0);
        assert_eq!(c.channels[1].sends_db[0], -6.0);
        assert_eq!(c.channels[0].sends_db[1], other);
        assert_eq!(remote.state.lock().unwrap().revision, 1);
        let p = Pending { epoch: 999, ..p };
        assert_eq!(
            apply(&remote, &p, &mut c, &ctl).unwrap_err().code,
            "topology"
        );
    }
}
pub(super) fn drain(remote: &Remote, c: &mut Config, ctl: &Controls, running: bool) {
    let mut replies = Vec::new();
    for _ in 0..32 {
        let Some(p) = remote.ws_queue.pop() else {
            break;
        };
        let result = apply(remote, &p, c, ctl);
        replies.push((p, result));
    }
    if !replies.is_empty() {
        remote.refresh(c, ctl, running);
    }
    for (p, result) in replies {
        let result = result.and_then(|_| {
            remote
                .view_for(&p.key, Some(p.aux), Some(&p.cookie))
                .map_err(view_error)
        });
        let _ = p.reply.try_send(result);
    }
}

fn would_block(error: &Error) -> bool {
    matches!(error, Error::Io(e) if e.kind() == std::io::ErrorKind::WouldBlock)
}
fn emit(socket: &mut WebSocket<PrefixStream>, value: Value) -> Result<(), Error> {
    socket.send(Message::Text(value.to_string().into()))
}

pub(super) fn serve(
    remote: &Remote,
    stream: TcpStream,
    raw: Vec<u8>,
    server_alive: Arc<AtomicBool>,
) -> std::io::Result<()> {
    let mut cookie = String::new();
    let callback = |request: &Request, response: Response| {
        let host = request
            .headers()
            .get("Host")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let origin = request
            .headers()
            .get("Origin")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if host.is_empty() || origin != format!("http://{host}") || request.uri().query().is_some()
        {
            return Err(tungstenite::http::Response::builder()
                .status(403)
                .body(Some("Origin denied".into()))
                .unwrap());
        }
        cookie = crate::access::cookie_value(
            request
                .headers()
                .get("Cookie")
                .and_then(|v| v.to_str().ok())
                .unwrap_or(""),
            "mm_mix",
        )
        .to_owned();
        Ok(response)
    };
    let config = WebSocketConfig::default()
        .read_buffer_size(4096)
        .write_buffer_size(0)
        .max_write_buffer_size(128 * 1024)
        .max_message_size(Some(MAX_BYTES))
        .max_frame_size(Some(MAX_BYTES));
    let mut socket = tungstenite::accept_hdr_with_config(
        PrefixStream {
            prefix: Cursor::new(raw),
            socket: stream,
        },
        callback,
        Some(config),
    )
    .map_err(|e| std::io::Error::other(e.to_string()))?;
    socket.get_mut().socket.set_nonblocking(true)?;
    let live = Arc::new(AtomicBool::new(true));
    let _lease = Lease(live.clone());
    let mut identity: Option<(String, usize, u64)> = None;
    let mut awaiting: VecDeque<(u64, mpsc::Receiver<Result<Value, Fault>>)> = VecDeque::new();
    let mut outgoing = VecDeque::new();
    let started = Instant::now();
    let mut last_received = started;
    let mut next_state = started;
    let mut next_ping = started + HEARTBEAT;
    let mut blocked_since = None;
    let mut last_seq = None;
    let mut rate_start = started;
    let mut rate = 0usize;
    let mut closing = false;
    while server_alive.load(Acquire) && remote.enabled.load(Acquire) {
        let now = Instant::now();
        if now.duration_since(last_received) >= DEAD_CONNECTION
            || (identity.is_none() && now.duration_since(started) >= Duration::from_secs(3))
        {
            break;
        }
        // Only one state is created after previous writes drain. Slow clients
        // receive a fresh snapshot, not an ever-growing queue of old meters.
        match socket.flush() {
            Ok(()) => {
                blocked_since = None;
            }
            Err(e) if would_block(&e) => {
                let since = blocked_since.get_or_insert(now);
                if now.duration_since(*since) >= Duration::from_secs(2) {
                    break;
                }
            }
            Err(_) => break,
        }
        if closing && outgoing.is_empty() && blocked_since.is_none() {
            break;
        }
        if !closing {
            for _ in 0..32 {
                let message = match socket.read() {
                    Ok(message) => message,
                    Err(e) if would_block(&e) => break,
                    Err(_) => {
                        closing = true;
                        break;
                    }
                };
                last_received = Instant::now();
                match message {
                    Message::Close(_) => {
                        live.store(false, Release);
                        closing = true;
                        break;
                    }
                    Message::Ping(_) | Message::Pong(_) => continue,
                    Message::Text(text) => {
                        if now.duration_since(rate_start) >= Duration::from_secs(1) {
                            rate_start = now;
                            rate = 0;
                        }
                        rate += 1;
                        if rate > 120 {
                            outgoing.push_back(
                                Fault::new("busy", "Demasiados mensajes de control")
                                    .wire(None, remote),
                            );
                            closing = true;
                            break;
                        }
                        let incoming: Incoming = match serde_json::from_str(&text) {
                            Ok(v) => v,
                            Err(_) => {
                                outgoing.push_back(
                                    Fault::new("invalid", "Mensaje de control no válido")
                                        .wire(None, remote),
                                );
                                continue;
                            }
                        };
                        match incoming {
                            Incoming::Hello { key, aux } if identity.is_none() => {
                                if key.len() > 128 {
                                    outgoing.push_back(
                                        Fault::new("auth", "Enlace inválido").wire(None, remote),
                                    );
                                    closing = true;
                                    break;
                                }
                                match remote.view_for(&key, aux, Some(&cookie)) {
                                    Ok(state) => {
                                        let aux = state["bus"]["index"].as_u64().unwrap() as usize;
                                        let epoch = state["epoch"].as_u64().unwrap();
                                        identity = Some((key, aux, epoch));
                                        outgoing.push_back(json!({"type":"state","state":state}));
                                        next_state = now + Duration::from_millis(100);
                                    }
                                    Err(e) => {
                                        outgoing.push_back(view_error(e).wire(None, remote));
                                        closing = true;
                                    }
                                }
                            }
                            Incoming::Change {
                                seq,
                                epoch,
                                aux,
                                changes,
                            } => {
                                let Some((key, selected_aux, initial_epoch)) = &identity else {
                                    outgoing.push_back(
                                        Fault::new("auth", "Envía hello antes de controlar")
                                            .wire(Some(seq), remote),
                                    );
                                    closing = true;
                                    break;
                                };
                                if epoch != *initial_epoch {
                                    outgoing.push_back(
                                        Fault::new(
                                            "topology",
                                            "La mezcla cambió; sincroniza de nuevo",
                                        )
                                        .wire(Some(seq), remote),
                                    );
                                    closing = true;
                                    break;
                                }
                                if aux.is_some_and(|a| a != *selected_aux)
                                    || changes.is_empty()
                                    || changes.len() > 64
                                    || last_seq.is_some_and(|previous| seq <= previous)
                                {
                                    outgoing.push_back(
                                        Fault::new(
                                            "invalid",
                                            "Secuencia, auxiliar o cantidad de cambios inválidos",
                                        )
                                        .wire(Some(seq), remote),
                                    );
                                    continue;
                                }
                                last_seq = Some(seq);
                                if awaiting.len() >= 8 {
                                    outgoing.push_back(
                                        Fault::new("busy", "Hay cambios sin confirmar")
                                            .wire(Some(seq), remote),
                                    );
                                    continue;
                                }
                                let (reply, receiver) = mpsc::sync_channel(1);
                                let pending = Pending {
                                    key: key.clone(),
                                    cookie: cookie.clone(),
                                    aux: *selected_aux,
                                    epoch,
                                    changes,
                                    deadline: now + Duration::from_millis(500),
                                    live: live.clone(),
                                    reply,
                                };
                                if remote.ws_queue.push(pending).is_err() {
                                    outgoing.push_back(
                                        Fault::new(
                                            "busy",
                                            "Control ocupado; el cambio no fue encolado",
                                        )
                                        .wire(Some(seq), remote),
                                    );
                                } else {
                                    awaiting.push_back((seq, receiver));
                                }
                            }
                            Incoming::Ping { sent } if identity.is_some() => {
                                if sent.is_number() || sent.is_null() {
                                    outgoing.push_back(json!({"type":"pong","sent":sent}));
                                }
                            }
                            _ => {
                                outgoing.push_back(
                                    Fault::new("invalid", "Mensaje inesperado").wire(None, remote),
                                );
                            }
                        }
                    }
                    _ => {
                        outgoing.push_back(
                            Fault::new("invalid", "Se requieren mensajes JSON de texto")
                                .wire(None, remote),
                        );
                        closing = true;
                    }
                }
                if outgoing.len() > 16 {
                    closing = true;
                    break;
                }
            }
        }
        while let Some((seq, receiver)) = awaiting.front() {
            let response = match receiver.try_recv() {
                Ok(Ok(state)) => json!({"type":"ack","seq":seq,"state":state}),
                Ok(Err(error)) => {
                    if error.code == "auth" || error.code == "topology" {
                        closing = true;
                    }
                    error.wire(Some(*seq), remote)
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(_) => {
                    Fault::new("busy", "El cambio no fue confirmado").wire(Some(*seq), remote)
                }
            };
            awaiting.pop_front();
            outgoing.push_back(response);
        }
        if !closing && blocked_since.is_none() && outgoing.is_empty() && now >= next_state {
            if let Some((key, aux, epoch)) = &identity {
                let topology_changed = remote.state.lock().unwrap().epoch != *epoch;
                let view = if topology_changed {
                    Err("La mezcla cambió; vuelve a conectar".into())
                } else {
                    remote.view_for(key, Some(*aux), Some(&cookie))
                };
                match view {
                    Ok(state) if state["epoch"].as_u64() == Some(*epoch) => {
                        outgoing.push_back(json!({"type":"state","state":state}))
                    }
                    Ok(_) => {
                        outgoing.push_back(
                            Fault::new("topology", "La mezcla cambió; vuelve a conectar")
                                .wire(None, remote),
                        );
                        closing = true;
                    }
                    Err(e) => {
                        let fault = if topology_changed {
                            Fault::new("topology", e)
                        } else {
                            view_error(e)
                        };
                        outgoing.push_back(fault.wire(None, remote));
                        closing = true;
                    }
                }
            }
            next_state = now + Duration::from_millis(100);
        }
        if outgoing.len() > 24 {
            break;
        }
        if blocked_since.is_none() {
            if let Some(response) = outgoing.pop_front() {
                match emit(&mut socket, response) {
                    Ok(()) => {}
                    Err(e) if would_block(&e) => {
                        blocked_since = Some(now);
                    }
                    Err(_) => break,
                }
            } else if !closing && identity.is_some() && now >= next_ping {
                match socket.send(Message::Ping(Vec::new().into())) {
                    Ok(()) => {}
                    Err(e) if would_block(&e) => {
                        blocked_since = Some(now);
                    }
                    Err(_) => break,
                }
                next_ping = now + HEARTBEAT;
            }
        }
        if closing {
            live.store(false, Release);
        }
        thread::sleep(Duration::from_millis(5));
    }
    live.store(false, Release);
    let _ = socket.close(None);
    Ok(())
}
