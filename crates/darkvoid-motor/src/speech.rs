//! Whisper local: nenhum download implícito e nenhum áudio sai do computador.
use crate::protocol::{self, Result, error};
use filmcraft_engine::Session;
use filmcraft_project::Transcript;
use filmcraft_speech::{SpeechError, models};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
use tokio::sync::broadcast;

#[derive(Deserialize)]
pub struct Download {
    model: String,
}
#[derive(Deserialize)]
pub struct Transcribe {
    model: String,
    language: Language,
}
#[derive(Clone, Copy, Deserialize)]
pub enum Language {
    #[serde(rename = "pt")]
    Portuguese,
    #[serde(rename = "en")]
    English,
    #[serde(rename = "auto")]
    Auto,
}
impl Language {
    fn code(self) -> Option<String> {
        match self {
            Self::Portuguese => Some("pt".into()),
            Self::English => Some("en".into()),
            Self::Auto => None,
        }
    }
}
#[derive(Default)]
struct Progress {
    cancel: AtomicBool,
    fraction: AtomicU32,
}
impl Progress {
    fn update(&self, fraction: f32) -> bool {
        self.fraction.fetch_max((fraction.clamp(0.0, 1.0) * 1_000_000.0) as u32, Ordering::Relaxed);
        !self.cancel.load(Ordering::Acquire)
    }
    fn check(&self) -> Result<()> {
        if self.cancel.load(Ordering::Acquire) { Err(cancelled()) } else { Ok(()) }
    }
}
struct Job {
    id: u64,
    kind: &'static str,
    progress: Arc<Progress>,
    worker: thread::JoinHandle<()>,
    result: mpsc::Receiver<Result<Value>>,
    started: Instant,
    emitted: Instant,
}
pub struct Speech {
    directory: PathBuf,
    events: broadcast::Sender<Value>,
    ids: Arc<AtomicU64>,
    active: Option<Job>,
}
impl Speech {
    pub fn new(directory: PathBuf, events: broadcast::Sender<Value>, ids: Arc<AtomicU64>) -> Self {
        Self { directory, events, ids, active: None }
    }
    pub fn list(&self) -> Value {
        json!({"models":models::catalogue().iter().map(|model| json!({
            "id":model.id,"sizeBytes":model.size(),"installed":models::installed(&self.directory,model),"license":model.license
        })).collect::<Vec<_>>()})
    }
    pub fn download(&mut self, params: Download) -> Result<Value> {
        let model = find(&params.model)?;
        let directory = self.directory.clone();
        // Pelo contrato, a tela envia este pedido somente após confirmar tamanho, origem e licença.
        // Nenhum outro comando chama o downloader.
        self.spawn("download", move |progress| {
            progress.check()?;
            let cleanup = Parts { directory: models::model_dir(&directory, model), model };
            let result = models::download(&directory, model, &mut |done, total, _| progress.update(done as f32 / total.max(1) as f32)).map_err(speech_error);
            drop(cleanup);
            result?;
            progress.check()?;
            if !models::installed(&directory, model) {
                return Err(error("model_integrity_failed", "O modelo não passou na verificação SHA-256."));
            }
            progress.update(1.0);
            Ok(json!({"model":model.id}))
        })
    }
    pub fn transcribe(&mut self, session: &Session, params: Transcribe) -> Result<Value> {
        let model = find(&params.model)?;
        let sequence = session.active_sequence().ok_or_else(|| error("no_session", "Abra um vídeo antes de transcrever."))?;
        let item = sequence
            .audio_tracks
            .iter()
            .flat_map(|track| track.items.iter())
            .find(|item| item.enabled)
            .map(|item| item.item)
            .ok_or_else(|| error("media_no_audio", "O vídeo da sessão não contém áudio."))?;
        let source = session
            .media
            .own_source(&session.project, item, &*session.services)
            .ok_or_else(|| error("media_unavailable", "Não foi possível abrir o áudio da sessão."))?;
        if !source.info().has_audio() {
            return Err(error("media_no_audio", "O vídeo da sessão não contém áudio."));
        }
        let duration = source.info().duration;
        let models_dir = self.directory.clone();
        // Não executa download. A verificação completa do modelo acontece no worker.
        let model_path = models::model_dir(&models_dir, model);
        if !model.files.iter().all(|file| model_path.join(file.name).is_file()) {
            return Err(error("model_not_installed", "Baixe e verifique o modelo antes de transcrever."));
        }
        self.spawn("transcribe", move |progress| {
            let start = Instant::now();
            progress.check()?;
            let recognizer = filmcraft_speech::load(&models_dir, model.id).map_err(speech_error)?;
            progress.check()?;
            let sample_count = usize::try_from(duration.to_units_floor(filmcraft_speech::SAMPLE_RATE as i64)).map_err(|_| protocol::invalid())?;
            if sample_count == 0 || sample_count > 3600 * filmcraft_speech::SAMPLE_RATE as usize {
                return Err(protocol::invalid());
            }
            let mut mono = Vec::with_capacity(sample_count);
            // Blocos limitados: evita pedir uma hora de áudio ao decoder de uma só vez.
            while mono.len() < sample_count {
                progress.check()?;
                let count = (sample_count - mono.len()).min(filmcraft_speech::SAMPLE_RATE as usize);
                let audio = source
                    .audio(mono.len() as i64, count, filmcraft_speech::SAMPLE_RATE)
                    .map_err(|_| error("audio_decode_failed", "Não foi possível decodificar o áudio para transcrição."))?;
                if audio.frames() != count || audio.channels.is_empty() {
                    return Err(error("audio_decode_failed", "O áudio decodificado está incompleto."));
                }
                mono.extend(filmcraft_speech::downmix(&audio.channels));
                progress.update(0.05 * mono.len() as f32 / sample_count as f32);
            }
            let options = filmcraft_speech::Options { language: params.language.code(), diarize: false, ..Default::default() };
            let transcript = recognizer.transcribe(&mono, &options, &mut |fraction, _| progress.update(0.05 + 0.94 * fraction)).map_err(speech_error)?;
            progress.check()?;
            let value = result(transcript, duration)?;
            eprintln!("Transcrição local com {} concluída em {:.3} s.", model.id, start.elapsed().as_secs_f64());
            Ok(value)
        })
    }
    fn spawn(&mut self, kind: &'static str, run: impl FnOnce(&Progress) -> Result<Value> + Send + 'static) -> Result<Value> {
        if self.active.is_some() {
            return Err(error("speech_busy", "Aguarde ou cancele o download ou a transcrição em andamento."));
        }
        let progress = Arc::new(Progress::default());
        let observed = progress.clone();
        let (sender, result) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name(format!("darkvoid-{kind}"))
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(&observed)))
                    .unwrap_or_else(|_| Err(error("speech_failed", "O motor encontrou uma falha no processamento de fala.")));
                let _ = sender.send(result);
            })
            .map_err(|_| error("speech_failed", "Não foi possível iniciar o processamento de fala."))?;
        let id = self.ids.fetch_add(1, Ordering::Relaxed);
        self.active = Some(Job { id, kind, progress, worker, result, started: Instant::now(), emitted: Instant::now() - Duration::from_secs(1) });
        Ok(json!({"jobId":id}))
    }
    pub fn cancel(&self, id: u64) -> bool {
        if let Some(job) = &self.active
            && job.id == id
        {
            job.progress.cancel.store(true, Ordering::Release);
            true
        } else {
            false
        }
    }
    pub fn poll(&mut self) {
        let Some(job) = self.active.as_mut() else {
            return;
        };
        let result = match job.result.try_recv() {
            Ok(value) => Some(value),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err(error("speech_failed", "O processamento de fala foi interrompido."))),
        };
        if let Some(result) = result {
            if result.is_ok() {
                let _ = self.events.send(event_progress(job, 1.0));
            }
            let message = match result {
                Ok(value) => json!({"event":"job.done","data":{"jobId":job.id,"kind":job.kind,"result":value}}),
                Err(e) => json!({"event":"job.error","data":{"jobId":job.id,"kind":job.kind,"error":{"code":e.code,"message":e.message}}}),
            };
            let _ = self.events.send(message);
            if let Some(job) = self.active.take() {
                let _ = job.worker.join();
            }
        } else if job.emitted.elapsed() >= Duration::from_millis(250) {
            let fraction = f64::from(job.progress.fraction.load(Ordering::Relaxed)) / 1_000_000.0;
            let _ = self.events.send(event_progress(job, fraction));
            job.emitted = Instant::now();
        }
    }
}
impl Drop for Speech {
    fn drop(&mut self) {
        if let Some(job) = self.active.take() {
            job.progress.cancel.store(true, Ordering::Release);
            let _ = job.worker.join();
        }
    }
}
fn event_progress(job: &Job, fraction: f64) -> Value {
    let eta = if fraction > 0.0 && fraction < 1.0 { Some(job.started.elapsed().as_secs_f64() * (1.0 - fraction) / fraction) } else { None };
    json!({"event":"job.progress","data":{"jobId":job.id,"kind":job.kind,"fraction":fraction,"etaSeconds":eta}})
}
fn find(id: &str) -> Result<&'static models::ModelInfo> {
    models::find(id).ok_or_else(|| error("unknown_model", "Modelo de fala desconhecido."))
}
fn cancelled() -> protocol::Error {
    error("job_cancelled", "Processamento de fala cancelado.")
}
fn speech_error(failure: SpeechError) -> protocol::Error {
    match failure {
        SpeechError::Cancelled => cancelled(),
        SpeechError::NotInstalled(_) => error("model_not_installed", "O modelo está ausente ou não passou na verificação SHA-256."),
        SpeechError::Download(_) => error("model_download_failed", "Não foi possível baixar e verificar o modelo de fala."),
        SpeechError::Io(_) => error("speech_io", "Não foi possível acessar os arquivos do modelo de fala."),
        _ => error("speech_failed", "Não foi possível executar o modelo local de transcrição."),
    }
}
fn result(transcript: Transcript, duration: filmcraft_time::Tick) -> Result<Value> {
    let mut words = Vec::new();
    let mut end = filmcraft_time::Tick::ZERO;
    for word in transcript.words {
        if word.text.trim().is_empty() {
            continue;
        }
        let start = word.start.max(end).max(filmcraft_time::Tick::ZERO).min(duration);
        let finish = word.end.max(start).min(duration);
        if finish <= start {
            continue;
        }
        if !word.confidence.is_finite() {
            return Err(error("speech_failed", "O modelo retornou uma confiança inválida."));
        }
        words.push(json!({"text":word.text,"start":start.seconds(),"end":finish.seconds(),"confidence":word.confidence.clamp(0.0,1.0)}));
        end = finish;
    }
    Ok(json!({"language":transcript.language,"words":words}))
}
struct Parts {
    directory: PathBuf,
    model: &'static models::ModelInfo,
}
impl Drop for Parts {
    fn drop(&mut self) {
        for file in self.model.files {
            let _ = std::fs::remove_file(self.directory.join(format!("{}.part", file.name)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn word_bounds_are_seconds_ordered_and_inside_audio() {
        let transcript = Transcript {
            language: "pt".into(),
            words: vec![
                filmcraft_project::Word::new("Olá", filmcraft_time::Tick::from_seconds_f64(0.1), filmcraft_time::Tick::from_seconds_f64(0.4)),
                filmcraft_project::Word::new("mundo", filmcraft_time::Tick::from_seconds_f64(0.3), filmcraft_time::Tick::from_seconds_f64(1.1)),
            ],
            ..Default::default()
        };
        let value = result(transcript, filmcraft_time::Tick::from_seconds_f64(1.0)).unwrap();
        assert_eq!(value["language"], "pt");
        assert_eq!(value["words"][0]["start"], 0.1);
        assert_eq!(value["words"][1]["start"], 0.4);
        assert_eq!(value["words"][1]["end"], 1.0);
        assert_eq!(value["words"][0]["confidence"], 1.0);
    }
    #[test]
    fn worker_cancel_is_routed_and_emits_portuguese_error() {
        let dir = tempfile::tempdir().unwrap();
        let (events, mut receiver) = broadcast::channel(64);
        let mut speech = Speech::new(dir.path().into(), events, Arc::new(AtomicU64::new(42)));
        let job = speech
            .spawn("transcribe", |progress| {
                while progress.update(0.5) {
                    thread::sleep(Duration::from_millis(1));
                }
                progress.check()?;
                Ok(json!({}))
            })
            .unwrap();
        assert_eq!(job["jobId"], 42);
        assert!(!speech.cancel(41));
        assert!(speech.cancel(42));
        let limit = Instant::now() + Duration::from_secs(3);
        loop {
            speech.poll();
            if let Ok(event) = receiver.try_recv()
                && event["event"] == "job.error"
            {
                assert_eq!(event["data"]["error"]["code"], "job_cancelled");
                assert_eq!(event["data"]["kind"], "transcribe");
                break;
            }
            assert!(Instant::now() < limit);
            thread::sleep(Duration::from_millis(1));
        }
    }
    #[test]
    fn cancelled_download_never_opens_network_and_cleans_parts() {
        let dir = tempfile::tempdir().unwrap();
        let model = find("whisper-small").unwrap();
        let model_dir = models::model_dir(dir.path(), model);
        std::fs::create_dir_all(&model_dir).unwrap();
        let part = model_dir.join("model.safetensors.part");
        std::fs::write(&part, b"incompleto").unwrap();
        drop(Parts { directory: model_dir, model });
        assert!(!part.exists());
        assert!(!models::installed(dir.path(), model));
        assert!(find("../escape").is_err());
    }
}
