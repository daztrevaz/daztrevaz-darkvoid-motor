use crate::protocol::{self, Result};
use filmcraft_engine::media_pool::PoolProvider;
use filmcraft_project::{ItemId, Project};
use filmcraft_render::RenderOptions;
use filmcraft_time::Tick;
use serde::{Deserialize, Serialize};
use std::{
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};
use tokio::sync::watch;

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub max_width: u32,
    pub max_height: u32,
    pub quality: u8,
}
impl Default for Config {
    fn default() -> Self {
        Self { max_width: 1920, max_height: 1080, quality: 80 }
    }
}
impl Config {
    pub fn validate(self) -> Result<Self> {
        if self.max_width == 0 || self.max_height == 0 || self.max_width > 3840 || self.max_height > 3840 || !(1..=100).contains(&self.quality) {
            return Err(protocol::invalid());
        }
        Ok(self)
    }
    pub fn scale(self, width: u32, height: u32) -> f32 {
        (self.max_width as f32 / width as f32).min(self.max_height as f32 / height as f32).min(1.0)
    }
}

#[derive(Clone)]
pub struct Frame {
    pub epoch: u64,
    pub time: Tick,
    pub bytes: Vec<u8>,
}
pub struct Job {
    pub project: Arc<Project>,
    pub sequence: ItemId,
    pub sources: PoolProvider,
    pub time: Tick,
    pub config: Config,
    pub epoch: u64,
}
#[derive(Default)]
pub struct Metrics {
    pub rendered: AtomicU64,
    pub dropped: AtomicU64,
    pub decode_us: AtomicU64,
    pub render_us: AtomicU64,
}
#[derive(Clone, Serialize)]
pub struct GpuInfo {
    name: String,
    backend: String,
}

type Slot = Arc<(Mutex<(Option<Job>, bool)>, Condvar)>;
pub struct Renderer {
    slot: Slot,
    worker: Option<std::thread::JoinHandle<()>>,
    pub gpu: Option<GpuInfo>,
    pub metrics: Arc<Metrics>,
}
impl Renderer {
    pub fn new(frames: watch::Sender<Option<Arc<Frame>>>, epoch: Arc<AtomicU64>) -> Self {
        let slot: Slot = Arc::new((Mutex::new((None, false)), Condvar::new()));
        let input = slot.clone();
        let metrics = Arc::new(Metrics::default());
        let stats = metrics.clone();
        let (ready, rx) = std::sync::mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            let mut gpu = gpu();
            let _ = ready.send(gpu.as_ref().map(|(_, info)| info.clone()));
            // Três encoders independentes; no máximo dois quadros aguardando JPEG.
            // O renderizador nunca espera um encoder ou cliente lento.
            let (encode, pending) = std::sync::mpsc::sync_channel::<RawFrame>(2);
            let pending = Arc::new(Mutex::new(pending));
            let encoders: Vec<_> = (0..3)
                .map(|_| {
                    let (pending, frames, epoch, stats) = (pending.clone(), frames.clone(), epoch.clone(), stats.clone());
                    std::thread::spawn(move || encode_frames(pending, frames, epoch, stats))
                })
                .collect();
            loop {
                let job = {
                    let (lock, wake) = &*input;
                    let Ok(mut value) = lock.lock() else {
                        break;
                    };
                    while value.0.is_none() && !value.1 {
                        value = match wake.wait(value) {
                            Ok(v) => v,
                            Err(_) => return,
                        };
                    }
                    if value.1 {
                        break;
                    }
                    value.0.take()
                };
                let Some(job) = job else {
                    continue;
                };
                if job.epoch != epoch.load(Ordering::Acquire) {
                    continue;
                }
                if let Some(frame) = render(&job, &mut gpu, &stats)
                    && encode.try_send(frame).is_err()
                {
                    stats.dropped.fetch_add(1, Ordering::Relaxed);
                }
            }
            drop(encode);
            for encoder in encoders {
                let _ = encoder.join();
            }
        });
        Self { slot, worker: Some(worker), gpu: rx.recv().unwrap_or(None), metrics }
    }
    pub fn submit(&self, job: Job) {
        let (lock, wake) = &*self.slot;
        if let Ok(mut value) = lock.lock() {
            if value.0.replace(job).is_some() {
                self.metrics.dropped.fetch_add(1, Ordering::Relaxed);
            }
            wake.notify_one();
        }
    }
}
impl Drop for Renderer {
    fn drop(&mut self) {
        let (lock, wake) = &*self.slot;
        if let Ok(mut value) = lock.lock() {
            value.1 = true;
            value.0 = None;
            wake.notify_one();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn gpu() -> Option<(filmcraft_gpu::GpuCompositor, GpuInfo)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(
        instance.request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::HighPerformance, ..Default::default() }),
    )
    .ok()?;
    let info = adapter.get_info();
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()?;
    Some((filmcraft_gpu::GpuCompositor::new(&device, &queue), GpuInfo { name: info.name, backend: format!("{:?}", info.backend) }))
}

struct RawFrame {
    w: u32,
    h: u32,
    rgba: Vec<u8>,
    time: Tick,
    epoch: u64,
    quality: u8,
    render_us: u64,
}

fn render(job: &Job, gpu: &mut Option<(filmcraft_gpu::GpuCompositor, GpuInfo)>, metrics: &Metrics) -> Option<RawFrame> {
    let seq = job.project.sequence(job.sequence)?;
    let opts = RenderOptions { scale: job.config.scale(seq.settings.width, seq.settings.height), captions: true, ..Default::default() };
    let start = Instant::now();
    let rgba = if let Some((compositor, _)) = gpu {
        let plan = filmcraft_render::plan::plan_frame(&job.project, job.sequence, job.time, opts, compositor.caps(), &job.sources);
        metrics.decode_us.store(start.elapsed().as_micros() as u64, Ordering::Relaxed);
        compositor.composite(&plan);
        compositor.read_output().map(|(w, h, px)| (w as usize, h as usize, px))
    } else {
        None
    };
    let (w, h, rgba) = rgba.unwrap_or_else(|| {
        let image = filmcraft_render::render_sequence_rgba8(&job.project, job.sequence, job.time, opts, &job.sources);
        metrics.decode_us.store(start.elapsed().as_micros() as u64, Ordering::Relaxed);
        image
    });
    let total = start.elapsed().as_micros() as u64;
    Some(RawFrame {
        w: w as u32,
        h: h as u32,
        rgba,
        time: job.time,
        epoch: job.epoch,
        quality: job.config.quality,
        render_us: total.saturating_sub(metrics.decode_us.load(Ordering::Relaxed)),
    })
}

fn encode_frames(
    pending: Arc<Mutex<std::sync::mpsc::Receiver<RawFrame>>>,
    frames: watch::Sender<Option<Arc<Frame>>>,
    epoch: Arc<AtomicU64>,
    stats: Arc<Metrics>,
) {
    loop {
        let raw = match pending.lock() {
            Ok(rx) => rx.recv(),
            Err(_) => return,
        };
        let Ok(mut raw) = raw else {
            return;
        };
        if raw.epoch != epoch.load(Ordering::Acquire) {
            continue;
        }
        let start = Instant::now();
        let Some(bytes) = jpeg(&mut raw) else {
            continue;
        };
        stats.render_us.store(raw.render_us + start.elapsed().as_micros() as u64, Ordering::Relaxed);
        let frame = Arc::new(Frame { epoch: raw.epoch, time: raw.time, bytes });
        // Encoders podem terminar fora de ordem. Nunca faça a prévia voltar no tempo.
        let sent = frames.send_if_modified(|current| {
            if raw.epoch != epoch.load(Ordering::Acquire) || current.as_ref().is_some_and(|f| f.epoch == raw.epoch && f.time >= raw.time) {
                return false;
            }
            *current = Some(frame);
            true
        });
        if sent {
            stats.rendered.fetch_add(1, Ordering::Relaxed);
        } else {
            stats.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

fn jpeg(raw: &mut RawFrame) -> Option<Vec<u8>> {
    let image = image::RgbaImage::from_raw(raw.w, raw.h, std::mem::take(&mut raw.rgba))?;
    let image = image::DynamicImage::ImageRgba8(image).into_rgb8();
    let mut bytes = header(raw.w, raw.h, raw.time);
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, raw.quality).encode(&image, raw.w, raw.h, image::ExtendedColorType::Rgb8).ok()?;
    Some(bytes)
}

fn header(w: u32, h: u32, time: Tick) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(24);
    bytes.extend_from_slice(b"DVF1");
    bytes.extend_from_slice(&w.to_le_bytes());
    bytes.extend_from_slice(&h.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&time.seconds().to_le_bytes());
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn binary_header_and_preview_limits() {
        let bytes = header(1920, 1080, Tick::from_seconds_f64(12.5));
        assert_eq!(bytes.len(), 24);
        assert_eq!(&bytes[..4], b"DVF1");
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 1920);
        assert_eq!(&bytes[12..16], &[0; 4]);
        assert_eq!(f64::from_le_bytes(bytes[16..24].try_into().unwrap()), 12.5);
        assert!(Config { max_width: 0, ..Default::default() }.validate().is_err());
        assert!(Config { quality: 101, ..Default::default() }.validate().is_err());
    }

    #[test]
    fn cpu_fallback_renders_video_through_the_engine() {
        use serde_json::json;
        let mut session = filmcraft_engine::Session::new(Arc::new(filmcraft_engine::FsServices));
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/padrao.mp4");
        let imported = session.execute("file.import", json!({"paths":[path]})).unwrap();
        session.execute("file.newSequenceFromClip", json!({"items":imported["items"]})).unwrap();
        let job = Job {
            project: session.project.clone(),
            sequence: session.state.active_sequence.unwrap(),
            sources: session.media.provider(session.project.clone(), session.services.clone()),
            time: Tick::from_seconds_f64(0.5),
            config: Config::default(),
            epoch: 1,
        };
        let mut raw = render(&job, &mut None, &Metrics::default()).unwrap();
        let bytes = jpeg(&mut raw).unwrap();
        let image = image::load_from_memory(&bytes[24..]).unwrap().into_rgb8();
        assert_eq!((image.width(), image.height()), (64, 36));
        assert!(image.pixels().any(|p| p.0[0] > 150 && p.0[1] < 80));
    }
}
