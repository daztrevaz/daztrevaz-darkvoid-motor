//! Um job de exportação por vez, com snapshot, cancelamento e publicação sem sobrescrita.
use crate::{
    encoding::{Capabilities, Encoder},
    protocol::{self, Result, error},
};
use filmcraft_engine::{MediaPool, Session};
use filmcraft_export::{ExportError, ExportSettings, Format, GpuRendering, HardwareEncoding, Progress};
use filmcraft_project::{ItemId, Project};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Seek, Write},
    path::{Path, PathBuf},
    sync::{Arc, atomic::Ordering, mpsc},
    thread,
    time::{Duration, Instant},
};
use tokio::sync::broadcast;

#[derive(Clone, Copy, Deserialize)]
pub enum Preset {
    #[serde(rename = "shorts_1080x1920")]
    Shorts,
    #[serde(rename = "youtube_1080p")]
    Youtube1080,
    #[serde(rename = "youtube_2160p")]
    Youtube2160,
}
impl Preset {
    fn settings(self) -> ExportSettings {
        let name = match self {
            Self::Shorts => "YouTube Shorts 1080×1920",
            Self::Youtube1080 => "YouTube 1080p Full HD",
            Self::Youtube2160 => "YouTube 2160p 4K Ultra HD",
        };
        filmcraft_export::presets::find_builtin(name).map(|preset| preset.settings).unwrap_or_default()
    }
}
#[derive(Deserialize)]
pub struct Start {
    path: String,
    preset: Preset,
    encoder: Encoder,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cancel {
    job_id: u64,
}

struct Running {
    id: u64,
    progress: Arc<Progress>,
    result: mpsc::Receiver<Result<Value>>,
    worker: thread::JoinHandle<()>,
    emitted_at: Instant,
    fraction: f64,
}
pub struct Exports {
    pub capabilities: Capabilities,
    cache: PathBuf,
    next_id: u64,
    active: Option<Running>,
    events: broadcast::Sender<Value>,
}
impl Exports {
    pub fn new(cache: PathBuf, events: broadcast::Sender<Value>) -> Self {
        Self { capabilities: Capabilities::detect(), cache, next_id: 1, active: None, events }
    }
    pub fn start(&mut self, session: &Session, params: Start) -> Result<Value> {
        if self.active.is_some() {
            return Err(error("export_busy", "Aguarde ou cancele a exportação em andamento."));
        }
        let sequence = session.state.active_sequence.ok_or_else(|| error("no_session", "Abra um vídeo antes de exportar."))?;
        let target = output_path(&params.path)?;
        let temp = tempfile::Builder::new()
            .prefix("export-")
            .suffix(".mp4")
            .tempfile_in(&self.cache)
            .map_err(|_| error("export_io", "Não foi possível criar o arquivo temporário da exportação."))?;
        let mut settings = params.preset.settings();
        self.capabilities.apply(params.encoder, &mut settings);
        settings.path = temp.path().to_string_lossy().into_owned();
        settings.output_file = Some(Arc::new(temp.as_file().try_clone().map_err(|_| io_error())?));
        settings.gpu_rendering = GpuRendering::Auto;
        settings.max_threads = 4;
        // A exportação contém a sessão inteira, independentemente do playhead.
        settings.range = session.active_sequence().map(|seq| filmcraft_time::TimeRange::new(filmcraft_time::Tick::ZERO, seq.duration()));
        let project = session.project.clone();
        let pool = Arc::new(MediaPool::default());
        let sources = pool.provider(project.clone(), session.services.clone()).own_sources();
        let progress = Arc::new(Progress::default());
        let observed = progress.clone();
        let (tx, result) = mpsc::sync_channel(1);
        let requested_path = params.path;
        let worker = thread::Builder::new()
            .name("darkvoid-export".into())
            .spawn(move || {
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run(&project, sequence, &mut settings, &sources, &observed)?;
                    // Libera todos os handles antes de publicar ou descartar o temporário no Windows.
                    settings.output_file.take();
                    if observed.cancel.load(Ordering::Acquire) {
                        return Err(cancelled());
                    }
                    let bytes = fs::metadata(temp.path()).map_err(|_| io_error())?.len();
                    publish(temp, &target, &observed)?;
                    Ok(json!({"path":requested_path,"bytes":bytes}))
                }))
                .unwrap_or_else(|_| Err(error("export_failed", "O motor encontrou uma falha durante a exportação.")));
                let _ = tx.send(outcome);
            })
            .map_err(|_| error("export_failed", "Não foi possível iniciar a exportação."))?;
        let id = self.next_id;
        self.next_id += 1;
        self.active = Some(Running { id, progress, result, worker, emitted_at: Instant::now() - Duration::from_secs(1), fraction: 0.0 });
        Ok(json!({"jobId":id}))
    }
    pub fn cancel(&self, params: Cancel) -> Result<Value> {
        let job =
            self.active.as_ref().filter(|job| job.id == params.job_id).ok_or_else(|| error("job_not_found", "Exportação em andamento não encontrada."))?;
        job.progress.cancel.store(true, Ordering::Release);
        Ok(json!({}))
    }
    pub fn poll(&mut self) {
        let Some(job) = self.active.as_mut() else {
            return;
        };
        let result = match job.result.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(error("export_failed", "A exportação foi interrompida inesperadamente."))),
            Err(mpsc::TryRecvError::Empty) => None,
        };
        if let Some(result) = result {
            let id = job.id;
            if result.is_ok() {
                let _ = self.events.send(progress_event(id, 1.0, None));
            }
            let event = match result {
                Ok(result) => json!({"event":"job.done","data":{"jobId":id,"kind":"export","result":result}}),
                Err(e) => json!({"event":"job.error","data":{"jobId":id,"kind":"export","error":{"code":e.code,"message":e.message}}}),
            };
            let _ = self.events.send(event);
            if let Some(job) = self.active.take() {
                let _ = job.worker.join();
            }
        } else if job.emitted_at.elapsed() >= Duration::from_millis(250) {
            job.fraction = job.fraction.max(f64::from(job.progress.fraction()).clamp(0.0, 1.0));
            let eta = job.progress.eta().map(|duration| duration.as_secs_f64());
            let _ = self.events.send(progress_event(job.id, job.fraction, eta));
            job.emitted_at = Instant::now();
        }
    }
}
impl Drop for Exports {
    fn drop(&mut self) {
        if let Some(job) = self.active.take() {
            job.progress.cancel.store(true, Ordering::Release);
            let _ = job.worker.join();
        }
    }
}
fn progress_event(id: u64, fraction: f64, eta: Option<f64>) -> Value {
    json!({"event":"job.progress","data":{"jobId":id,"kind":"export","fraction":fraction,"etaSeconds":eta}})
}
fn publish(temp: tempfile::NamedTempFile, target: &Path, progress: &Progress) -> Result<()> {
    match temp.persist_noclobber(target) {
        Ok(_) => Ok(()),
        Err(failure) if failure.error.kind() == std::io::ErrorKind::CrossesDevices || (cfg!(windows) && failure.error.raw_os_error() == Some(17)) => {
            // Cache e destino podem estar em discos diferentes. Grave somente no caminho pedido.
            copy_to_target(failure.file, target, progress)
        }
        Err(_) => Err(error("export_publish_failed", "Não foi possível salvar a exportação. O destino pode já existir.")),
    }
}
fn copy_to_target(mut temp: tempfile::NamedTempFile, target: &Path, progress: &Progress) -> Result<()> {
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)
        .map_err(|_| error("export_publish_failed", "Não foi possível criar a exportação sem sobrescrever o destino."))?;
    let result = (|| {
        temp.as_file_mut().rewind().map_err(|_| io_error())?;
        let mut buffer = vec![0; 1024 * 1024];
        loop {
            if progress.cancel.load(Ordering::Acquire) {
                return Err(cancelled());
            }
            let count = temp.read(&mut buffer).map_err(|_| io_error())?;
            if count == 0 {
                break;
            }
            output.write_all(&buffer[..count]).map_err(|_| io_error())?;
        }
        output.sync_all().map_err(|_| io_error())
    })();
    drop(output);
    if result.is_err() {
        let _ = fs::remove_file(target);
    }
    result
}
fn output_path(value: &str) -> Result<PathBuf> {
    if value.contains("://") || value.starts_with(r"\\") || value.starts_with("//") {
        return Err(protocol::invalid());
    }
    let path = Path::new(value);
    if path.extension().is_none_or(|ext| !ext.eq_ignore_ascii_case("mp4")) {
        return Err(protocol::invalid());
    }
    match fs::symlink_metadata(path) {
        Ok(_) => return Err(error("output_exists", "O arquivo de destino já existe. Escolha outro nome.")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(io_error()),
    }
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    let parent = fs::canonicalize(parent).map_err(|_| error("export_directory_missing", "A pasta de destino não existe ou não está acessível."))?;
    if parent.to_string_lossy().starts_with(r"\\?\UNC\") {
        return Err(protocol::invalid());
    }
    Ok(parent.join(path.file_name().ok_or_else(protocol::invalid)?))
}
fn run(
    project: &Arc<Project>,
    sequence: ItemId,
    settings: &mut ExportSettings,
    sources: &dyn filmcraft_render::SourceProvider,
    progress: &Progress,
) -> Result<()> {
    let hardware = settings.hardware_encoding == HardwareEncoding::Auto;
    let mut result = filmcraft_export::export(project, sequence, settings, sources, progress);
    if hardware && matches!(result, Err(ExportError::Encode(_) | ExportError::Unsupported(_))) && !progress.cancel.load(Ordering::Acquire) {
        eprintln!("Codificador de hardware falhou; a exportação será reiniciada por software.");
        let mut file = settings.output_file.as_ref().ok_or_else(io_error)?.as_ref();
        file.set_len(0).map_err(|_| io_error())?;
        file.rewind().map_err(|_| io_error())?;
        file.flush().map_err(|_| io_error())?;
        settings.format = Format::H264;
        settings.hardware_encoding = HardwareEncoding::Off;
        progress.done.store(0, Ordering::Relaxed);
        result = filmcraft_export::export(project, sequence, settings, sources, progress);
    }
    result
        .map(|report| {
            eprintln!("Exportação concluída em {:.3} s; codificador: {}.", report.seconds, report.encoder.as_deref().unwrap_or("software"));
        })
        .map_err(|failure| match failure {
            ExportError::Cancelled => cancelled(),
            ExportError::Io(_) => io_error(),
            _ => error("export_failed", "Não foi possível codificar o vídeo e o áudio da exportação."),
        })
}
fn cancelled() -> protocol::Error {
    error("job_cancelled", "Exportação cancelada.")
}
fn io_error() -> protocol::Error {
    error("export_io", "Não foi possível ler ou gravar os arquivos da exportação.")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn presets_keep_engine_delivery_settings() {
        for (preset, size) in [(Preset::Shorts, (1080, 1920)), (Preset::Youtube1080, (1920, 1080)), (Preset::Youtube2160, (3840, 2160))] {
            let settings = preset.settings();
            assert_eq!(settings.frame_size, Some(size));
            assert!(settings.include_audio);
            assert_eq!(settings.format, Format::H264);
            settings.validate().unwrap();
        }
    }
    #[test]
    fn output_validation_never_overwrites_existing_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("existente.mp4");
        fs::write(&path, b"preservar").unwrap();
        assert_eq!(output_path(path.to_str().unwrap()).unwrap_err().code, "output_exists");
        assert_eq!(fs::read(path).unwrap(), b"preservar");
        assert!(output_path("https://example.invalid/video.mp4").is_err());
        assert!(output_path(dir.path().join("video.mov").to_str().unwrap()).is_err());
        assert!(output_path(dir.path().join("nova/video.mp4").to_str().unwrap()).is_err());
    }
    #[test]
    fn copying_between_disks_preserves_data_and_cleans_cancelled_outputs() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("video.mp4");
        let source = || {
            let mut file = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
            file.write_all(b"conteudo completo").unwrap();
            file
        };
        let progress = Progress::default();
        copy_to_target(source(), &target, &progress).unwrap();
        assert!(copy_to_target(source(), &target, &progress).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"conteudo completo");
        let cancelled_target = dir.path().join("cancelado.mp4");
        progress.cancel.store(true, Ordering::Relaxed);
        assert_eq!(copy_to_target(source(), &cancelled_target, &progress).unwrap_err().code, "job_cancelled");
        assert!(!cancelled_target.exists());
    }

    #[test]
    fn hardware_failure_restarts_a_clean_software_bitstream() {
        fn broken_hardware(
            format: Format,
            _: u32,
            _: u32,
            _: filmcraft_time::FrameRate,
            settings: &ExportSettings,
        ) -> Option<filmcraft_export::Result<Box<dyn filmcraft_export::VideoEncoder>>> {
            if format != Format::H264 || settings.hardware_encoding != HardwareEncoding::Auto || !settings.path.contains("dv-fallback-test-") {
                return None;
            }
            let mut file = settings.output_file.as_ref().unwrap().as_ref();
            file.write_all(b"bitstream parcial invalido").unwrap();
            Some(Err(ExportError::Encode("falha de hardware simulada".into())))
        }
        filmcraft_export::register_encoder(broken_hardware);
        let mut session = Session::new(Arc::new(filmcraft_engine::FsServices));
        let input = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/exportacao.mp4");
        let imported = session.execute("file.import", json!({"paths":[input]})).unwrap();
        session.execute("file.newSequenceFromClip", json!({"items":imported["items"]})).unwrap();
        let output = tempfile::Builder::new().prefix("dv-fallback-test-").suffix(".mp4").tempfile().unwrap();
        let mut settings = ExportSettings {
            path: output.path().to_string_lossy().into_owned(),
            hardware_encoding: HardwareEncoding::Auto,
            output_file: Some(Arc::new(output.as_file().try_clone().unwrap())),
            ..Default::default()
        };
        let sources = session.media.provider(session.project.clone(), session.services.clone());
        run(&session.project, session.state.active_sequence.unwrap(), &mut settings, &sources, &Progress::default()).unwrap();
        assert_eq!(settings.hardware_encoding, HardwareEncoding::Off);
        let info = session.media.probe_file(&settings.path, &*session.services).unwrap();
        assert_eq!(info.info().video.as_ref().unwrap().width, 128);
        assert!(info.info().has_audio());
        let bytes = fs::read(output.path()).unwrap();
        assert_eq!(&bytes[4..8], b"ftyp");
    }
}
