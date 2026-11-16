//! Miniaturas em worker limitado, com fontes independentes e cache persistente.
use crate::{
    cache::Cache,
    protocol::{self, Result, error},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use filmcraft_engine::{FsServices, MediaPool};
use filmcraft_media::{FrameRequest, MediaKind, MediaSource};
use filmcraft_time::Tick;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::hash_map::DefaultHasher,
    fs::{self, File},
    hash::{Hash, Hasher},
    io::BufReader,
    ops::Range,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
};
use tokio::sync::oneshot;

#[derive(Deserialize)]
struct Params {
    media: String,
    count: usize,
    height: u32,
}
#[derive(Serialize, Deserialize)]
struct Thumb {
    time: f64,
    jpeg: String,
}
struct Request {
    params: Params,
    id: Value,
    reply: oneshot::Sender<Value>,
}
pub struct Thumbs {
    queue: Option<mpsc::SyncSender<Request>>,
    worker: Option<thread::JoinHandle<()>>,
    stop: Arc<AtomicBool>,
}
fn failed() -> protocol::Error {
    error("media_thumbs_failed", "Não foi possível gerar as miniaturas do vídeo.")
}
fn invalid() -> protocol::Error {
    error("invalid_params", "Informe um vídeo local, count entre 1 e 200 e height entre 32 e 240.")
}
impl Thumbs {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Release);
    }
    pub fn new(cache: Cache) -> Self {
        let (queue, pending) = mpsc::sync_channel::<Request>(4);
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let worker = thread::Builder::new()
            .name("darkvoid-thumbs".into())
            .spawn(move || {
                while let Ok(request) = pending.recv() {
                    if stopped.load(Ordering::Acquire) {
                        break;
                    }
                    if request.reply.is_closed() {
                        continue;
                    }
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| calculate(&cache, &request.params, &stopped)))
                        .unwrap_or_else(|_| Err(failed()));
                    let _ = request.reply.send(protocol::response(request.id, result));
                }
            })
            .ok();
        Self { queue: worker.as_ref().map(|_| queue), worker, stop }
    }
    pub fn request(&self, params: Value, id: Value, reply: oneshot::Sender<Value>) {
        let parsed = serde_json::from_value::<Params>(params).map_err(|_| invalid()).and_then(|p| {
            if !(1..=200).contains(&p.count)
                || !(32..=240).contains(&p.height)
                || p.media.contains("://")
                || p.media.starts_with(r"\\")
                || p.media.starts_with("//")
                || p.media.is_empty()
            {
                return Err(invalid());
            }
            Ok(p)
        });
        let params = match parsed {
            Ok(p) => p,
            Err(e) => {
                let _ = reply.send(protocol::response(id, Err(e)));
                return;
            }
        };
        let request = Request { params, id, reply };
        let Some(queue) = &self.queue else {
            let _ = request.reply.send(protocol::response(request.id, Err(failed())));
            return;
        };
        if let Err(e) = queue.try_send(request) {
            let (r, error) = match e {
                mpsc::TrySendError::Full(r) => (r, error("media_thumbs_busy", "Aguarde a geração de miniaturas em andamento.")),
                mpsc::TrySendError::Disconnected(r) => (r, failed()),
            };
            let _ = r.reply.send(protocol::response(r.id, Err(error)));
        }
    }
}
impl Drop for Thumbs {
    fn drop(&mut self) {
        self.stop();
        self.queue.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn fingerprint(params: &Params) -> Result<u64> {
    let path = fs::canonicalize(&params.media).map_err(|_| error("media_not_found", "Arquivo de vídeo não encontrado."))?;
    let metadata = fs::metadata(&path).map_err(|_| failed())?;
    if !metadata.is_file() {
        return Err(error("unsupported_media", "Selecione um arquivo de vídeo."));
    }
    let mut key = DefaultHasher::new();
    ("thumbs-v1", path, metadata.len(), metadata.modified().map_err(|_| failed())?, metadata.created().ok(), params.count, params.height).hash(&mut key);
    Ok(key.finish())
}
fn calculate(cache: &Cache, params: &Params, stop: &Arc<AtomicBool>) -> Result<Value> {
    let key = fingerprint(params)?;
    cache.entry(&format!("thumbs-v1-{key:016x}.json"), |path| {
        if let Ok(file) = File::open(path)
            && file.metadata().is_ok_and(|m| m.len() <= 64 * 1024 * 1024)
            && let Ok(thumbs) = serde_json::from_reader::<_, Vec<Thumb>>(BufReader::new(file))
            && thumbs.len() == params.count
            && thumbs.iter().all(|t| t.time.is_finite() && t.time >= 0.0 && t.jpeg.starts_with("/9j/"))
        {
            return Ok(json!({"thumbs":thumbs}));
        }
        let source = MediaPool::default()
            .probe_file(&params.media, &FsServices)
            .map_err(|_| error("unsupported_media", "Não foi possível abrir o vídeo para gerar miniaturas."))?;
        if source.info().kind != MediaKind::Movie || source.info().video.is_none() || source.info().duration <= Tick::ZERO {
            return Err(error("unsupported_media", "Miniaturas exigem um vídeo com duração maior que zero."));
        }
        let duration = source.info().duration;
        drop(source);
        let workers = thread::available_parallelism().map_or(1, |n| (n.get() / 2).clamp(1, 6));
        let batch = params.count.div_ceil(workers);
        let thumbs = thread::scope(|scope| {
            let workers: Vec<_> = (0..params.count)
                .step_by(batch)
                .map(|start| scope.spawn(move || generate(params, duration, start..(start + batch).min(params.count), stop)))
                .collect();
            let mut thumbs = Vec::with_capacity(params.count);
            for worker in workers {
                thumbs.extend(worker.join().map_err(|_| failed())??);
            }
            Ok::<_, protocol::Error>(thumbs)
        })?;
        if fingerprint(params)? != key {
            return Err(error("media_changed", "O vídeo mudou durante a geração. Solicite as miniaturas novamente."));
        }
        let mut temp = tempfile::NamedTempFile::new_in(path.parent().ok_or_else(failed)?).map_err(|_| failed())?;
        serde_json::to_writer(temp.as_file_mut(), &thumbs).map_err(|_| failed())?;
        temp.as_file().sync_all().map_err(|_| failed())?;
        temp.persist(path).map_err(|_| failed())?;
        Ok(json!({"thumbs":thumbs}))
    })
}

fn generate(params: &Params, duration: Tick, indices: Range<usize>, stop: &Arc<AtomicBool>) -> Result<Vec<Thumb>> {
    filmcraft_media::cancel::with_background(true, || {
        filmcraft_media::cancel::with_cancel(stop, || {
            let source = MediaPool::default().probe_file(&params.media, &FsServices).map_err(|_| failed())?;
            indices
                .map(|index| {
                    if stop.load(Ordering::Acquire) {
                        return Err(failed());
                    }
                    let time = Tick((duration.0 as i128 * index as i128 / params.count as i128) as i64);
                    thumbnail(&*source, params.height, time)
                })
                .collect()
        })
    })
}

fn thumbnail(source: &dyn MediaSource, height: u32, time: Tick) -> Result<Thumb> {
    let scale = height as f32 / source.info().video.as_ref().ok_or_else(failed)?.height as f32;
    let frame = source.video_frame(FrameRequest { time, scale: scale.min(1.0) }).map_err(|_| failed())?;
    let width = (height as f64 * frame.width as f64 / frame.height as f64 * frame.par.0 as f64 / frame.par.1.max(1) as f64).round().max(1.0);
    if width > 16384.0 {
        return Err(error("unsupported_media", "A proporção do vídeo excede o tamanho seguro de uma miniatura."));
    }
    let n = (frame.height / height).max(1) as usize;
    let reduced = frame.box_decimated(n);
    let frame = reduced.as_ref().unwrap_or(&frame);
    let image = image::RgbaImage::from_raw(frame.width, frame.height, frame.to_rgba8()).ok_or_else(failed)?;
    let image = image::DynamicImage::ImageRgba8(image).into_rgb8();
    let resized = image::imageops::resize(&image, width as u32, height, image::imageops::FilterType::Triangle);
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 75).encode_image(&resized).map_err(|_| failed())?;
    Ok(Thumb { time: time.seconds(), jpeg: STANDARD.encode(jpeg) })
}
