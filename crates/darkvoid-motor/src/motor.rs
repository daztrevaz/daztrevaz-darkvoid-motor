use crate::{
    VERSION,
    audio::Audio,
    preview::{self, Frame, Renderer},
    protocol::{self, Result, error, field},
};
use filmcraft_engine::{FsServices, Session};
use filmcraft_media::MediaInfo;
use filmcraft_time::Tick;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, oneshot, watch};

pub enum Command {
    Request(u64, protocol::Request, oneshot::Sender<Value>),
    Disconnect(u64),
}
#[derive(Clone)]
pub struct Updates {
    pub frames: watch::Sender<Option<Arc<Frame>>>,
    pub state: watch::Sender<Value>,
    pub stats: watch::Sender<Value>,
    pub epoch: Arc<AtomicU64>,
    pub active_client: Arc<AtomicU64>,
}
impl Updates {
    pub fn new() -> Self {
        Self {
            frames: watch::channel(None).0,
            state: watch::channel(json!({"event":"state", "data":{"playing":false,"seconds":0.0}})).0,
            stats: watch::channel(json!({})).0,
            epoch: Arc::new(AtomicU64::new(0)),
            active_client: Arc::new(AtomicU64::new(0)),
        }
    }
}

pub fn run(mut commands: mpsc::Receiver<Command>, updates: Updates, stopped: watch::Receiver<bool>) {
    let renderer = Renderer::new(updates.frames.clone(), updates.epoch.clone());
    let mut motor = Motor {
        session: Session::new(Arc::new(FsServices)),
        config: preview::Config::default(),
        renderer,
        audio: None,
        updates,
        time: Tick::ZERO,
        last_frame: -1,
    };
    let mut state_at = Instant::now();
    let mut stats_at = Instant::now();
    let mut rendered = 0;
    loop {
        if *stopped.borrow() {
            break;
        }
        match commands.try_recv() {
            Ok(Command::Request(client, request, reply)) if client == motor.updates.active_client.load(Ordering::Acquire) => {
                let result = motor.execute(&request.cmd, request.params);
                let _ = reply.send(protocol::response(request.id, result));
            }
            Ok(Command::Disconnect(client)) if client == motor.updates.active_client.load(Ordering::Acquire) => {
                motor.pause();
                motor.emit_state();
            }
            Ok(_) => {}
            Err(mpsc::error::TryRecvError::Disconnected) => break,
            Err(mpsc::error::TryRecvError::Empty) => std::thread::sleep(Duration::from_millis(2)),
        }
        if motor.audio.is_none() {
            stats_at = Instant::now();
            rendered = motor.renderer.metrics.rendered.load(Ordering::Relaxed);
            continue;
        }
        motor.advance();
        if state_at.elapsed() >= Duration::from_secs_f64(1.0 / 30.0) {
            motor.emit_state();
            state_at = Instant::now();
        }
        if stats_at.elapsed() >= Duration::from_secs(1) {
            let metrics = &motor.renderer.metrics;
            let count = metrics.rendered.load(Ordering::Relaxed);
            motor.updates.stats.send_replace(json!({"event":"stats", "data":{
                "fps": (count - rendered) as f64 / stats_at.elapsed().as_secs_f64(),
                "droppedFrames":metrics.dropped.load(Ordering::Relaxed),
                "decodeMs":metrics.decode_us.load(Ordering::Relaxed) as f64 / 1000.0,
                "renderMs":metrics.render_us.load(Ordering::Relaxed) as f64 / 1000.0
            }}));
            rendered = count;
            stats_at = Instant::now();
        }
    }
    motor.pause();
}

struct Motor {
    session: Session,
    config: preview::Config,
    renderer: Renderer,
    audio: Option<Audio>,
    updates: Updates,
    time: Tick,
    last_frame: i64,
}
impl Motor {
    fn execute(&mut self, command: &str, params: Value) -> Result<Value> {
        match command {
            "hello" => Ok(json!({"version":VERSION,"gpu":self.renderer.gpu,"encoders":["software"],"speech":false})),
            "media.probe" => {
                let p: Path = field(params)?;
                let info = self.probe(&p.path)?;
                Ok(probe_result(&info))
            }
            "session.open" => self.open(field(params)?),
            "preview.config" => {
                let config: preview::Config = field(params)?;
                self.config = config.validate()?;
                self.invalidate();
                self.submit();
                Ok(json!({}))
            }
            "transport.play" => {
                let duration = self.duration()?;
                if self.audio.is_none() {
                    if self.time >= duration {
                        self.time = Tick::ZERO;
                        self.invalidate();
                    }
                    self.audio = Some(Audio::start(&self.session, self.time)?);
                    self.last_frame = -1;
                }
                self.emit_state();
                Ok(json!({}))
            }
            "transport.pause" => {
                self.duration()?;
                self.pause();
                self.emit_state();
                Ok(json!({}))
            }
            "transport.seek" => self.seek(field(params)?),
            "shutdown" => {
                self.pause();
                Ok(json!({}))
            }
            _ => Err(error("unknown_command", "Comando não disponível nesta versão do motor.")),
        }
    }
    fn probe(&self, path: &str) -> Result<MediaInfo> {
        // Recusa URLs e caminhos de rede: toda mídia deve vir do disco local.
        if path.contains("://") || path.starts_with(r"\\") || path.starts_with("//") {
            return Err(protocol::invalid());
        }
        if !std::path::Path::new(path).is_file() {
            return Err(error("media_not_found", "Arquivo não encontrado."));
        }
        self.session
            .media
            .probe_file(path, &*self.session.services)
            .map(|s| s.info().clone())
            .map_err(|_| error("media_unsupported", "Não foi possível abrir esta mídia."))
    }
    fn open(&mut self, params: Open) -> Result<Value> {
        if !["source", "vertical", "horizontal"].contains(&params.format.as_str()) {
            return Err(protocol::invalid());
        }
        let info = self.probe(&params.path)?;
        if info.video.is_none() || info.duration <= Tick::ZERO || info.duration > Tick::from_seconds_f64(3600.0) {
            return Err(error("media_unsupported", "Selecione um vídeo com duração de até uma hora."));
        }
        let mut next = Session::new(self.session.services.clone());
        next.prefs.media.import_image_sequences = false;
        let imported = next.execute("file.import", json!({"paths":[params.path]})).map_err(engine_error)?;
        let item = imported["items"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(Value::as_u64)
            .ok_or_else(|| error("media_unsupported", "Não foi possível importar o vídeo."))?;
        let created = next.execute("file.newSequenceFromClip", json!({"items":[item]})).map_err(engine_error)?;
        let settings = match params.format.as_str() {
            "vertical" => json!({"width":1080,"height":1920}),
            "horizontal" => json!({"width":1920,"height":1080}),
            _ => json!({}),
        };
        next.execute("sequence.settings", settings).map_err(engine_error)?;
        next.execute("clip.fitToFrame", json!({"clips":created["clips"]})).map_err(engine_error)?;
        let seq = next.active_sequence().ok_or_else(no_session)?;
        let result = json!({"width":seq.settings.width,"height":seq.settings.height,"fps":seq.settings.frame_rate.as_f64(),"duration":info.duration.seconds()});
        self.pause();
        self.session = next;
        self.time = Tick::ZERO;
        self.invalidate();
        self.submit();
        self.emit_state();
        Ok(result)
    }
    fn duration(&self) -> Result<Tick> {
        self.session.active_sequence().map(|s| s.duration()).ok_or_else(no_session)
    }
    fn seek(&mut self, params: Seek) -> Result<Value> {
        let duration = self.duration()?;
        if !params.seconds.is_finite() || params.seconds < 0.0 || params.seconds > duration.seconds() {
            return Err(protocol::invalid());
        }
        let playing = self.audio.is_some();
        self.pause();
        self.time = Tick::from_seconds_f64(params.seconds).min(duration);
        self.invalidate();
        self.submit();
        let result = if playing && self.time < duration { Audio::start(&self.session, self.time).map(|audio| self.audio = Some(audio)) } else { Ok(()) };
        self.emit_state();
        result?;
        Ok(json!({}))
    }
    fn invalidate(&mut self) {
        self.updates.epoch.fetch_add(1, Ordering::AcqRel);
        self.updates.frames.send_replace(None);
        self.last_frame = -1;
    }
    fn submit(&self) {
        let Some(sequence) = self.session.state.active_sequence else {
            return;
        };
        let duration = self.duration().unwrap_or(Tick::ZERO);
        // A sequência é semiaberta. No fim, mostre o último quadro, não preto.
        let time = self.time.min(Tick(duration.0.saturating_sub(1)).max(Tick::ZERO));
        self.renderer.submit(preview::Job {
            project: self.session.project.clone(),
            sequence,
            sources: self.session.media.provider(self.session.project.clone(), self.session.services.clone()),
            time,
            config: self.config,
            epoch: self.updates.epoch.load(Ordering::Acquire),
        });
    }
    fn advance(&mut self) {
        let Some(audio) = &self.audio else {
            return;
        };
        let duration = self.duration().unwrap_or(Tick::ZERO);
        self.time = audio.time().min(duration);
        if audio.failed.load(Ordering::Acquire) {
            eprintln!("A reprodução foi pausada porque o dispositivo de áudio falhou.");
            self.pause();
            self.emit_state();
            return;
        }
        if self.time >= duration {
            self.pause();
            self.emit_state();
            self.submit();
            return;
        }
        let rate = self.session.sequence_rate();
        let frame = rate.frame_at(self.time);
        if frame != self.last_frame {
            if self.last_frame >= 0 && frame > self.last_frame + 1 {
                self.renderer.metrics.dropped.fetch_add((frame - self.last_frame - 1) as u64, Ordering::Relaxed);
            }
            self.last_frame = frame;
            self.submit();
        }
    }
    fn pause(&mut self) {
        if let Some(audio) = self.audio.take() {
            self.time = audio.time().min(self.duration().unwrap_or(Tick::ZERO));
        }
    }
    fn emit_state(&self) {
        self.updates.state.send_replace(json!({"event":"state","data":{"playing":self.audio.is_some(),"seconds":self.time.seconds()}}));
    }
}
fn engine_error(_: filmcraft_engine::EngineError) -> protocol::Error {
    error("engine_error", "Não foi possível preparar a sessão de vídeo.")
}
fn no_session() -> protocol::Error {
    error("no_session", "Abra um vídeo antes de usar a reprodução.")
}

#[derive(Deserialize)]
struct Path {
    path: String,
}
#[derive(Deserialize)]
struct Open {
    path: String,
    format: String,
}
#[derive(Deserialize)]
struct Seek {
    seconds: f64,
}
fn probe_result(info: &MediaInfo) -> Value {
    json!({"duration":info.duration.seconds(),"width":info.video.as_ref().map(|v| v.width).unwrap_or(0),
        "height":info.video.as_ref().map(|v| v.height).unwrap_or(0),"fps":info.frame_rate().as_f64(),"hasAudio":info.has_audio(),
        "videoCodec":info.video.as_ref().map(|v| &v.codec),"audioCodec":info.audio().map(|a| &a.codec)})
}
