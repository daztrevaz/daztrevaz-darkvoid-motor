//! Mixagem fora do callback; callback sem locks, decodificação ou alocações.
use crate::protocol::{Result, error};
use cpal::{
    FromSample, SizedSample,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use crossbeam_queue::ArrayQueue;
use filmcraft_engine::{MediaPool, Session};
use filmcraft_project::{ItemId, Project};
use filmcraft_time::Tick;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::Duration,
};

pub struct Audio {
    projects: tokio::sync::watch::Sender<Arc<Project>>,
    stream: Option<cpal::Stream>,
    producer: Option<thread::JoinHandle<()>>,
    stop: Arc<AtomicBool>,
    pub failed: Arc<AtomicBool>,
    clock: Arc<AtomicU64>,
    underruns: Arc<AtomicU64>,
    rate: u32,
    origin: Tick,
}

impl Audio {
    pub fn start(session: &Session, origin: Tick) -> Result<Self> {
        let device = cpal::default_host().default_output_device().ok_or_else(unavailable)?;
        let config = device.default_output_config().map_err(|_| unavailable())?;
        let rate = config.sample_rate().0;
        let sequence = session.state.active_sequence.ok_or_else(unavailable)?;
        let mut project = (*session.project).clone();
        project.sequence_mut(sequence).ok_or_else(unavailable)?.settings.sample_rate = rate;
        let mut project = Arc::new(project);
        // Pool independente: a imagem nunca segura o decoder usado pelo áudio.
        let pool = Arc::new(MediaPool::default());
        let services = session.services.clone();
        let mut sources = pool.provider(project.clone(), services.clone());
        let queue = Arc::new(ArrayQueue::new(rate as usize / 2));
        let mut cursor = origin.to_units_floor(rate as i64);
        fill(&project, sequence, &sources, &queue, &mut cursor, rate as usize / 5);
        let clock = Arc::new(AtomicU64::new(0));
        let failed = Arc::new(AtomicBool::new(false));
        let underruns = Arc::new(AtomicU64::new(0));
        let input = Callback {
            queue: queue.clone(),
            clock: clock.clone(),
            failed: failed.clone(),
            underruns: underruns.clone(),
            submitted: 0,
            rate,
            channels: config.channels() as usize,
        };
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => build::<f32>(&device, &config.into(), input),
            cpal::SampleFormat::I16 => build::<i16>(&device, &config.into(), input),
            cpal::SampleFormat::U16 => build::<u16>(&device, &config.into(), input),
            cpal::SampleFormat::I32 => build::<i32>(&device, &config.into(), input),
            cpal::SampleFormat::F64 => build::<f64>(&device, &config.into(), input),
            _ => return Err(error("audio_format_unsupported", "Formato do dispositivo de áudio não suportado.")),
        }?;
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let (projects, mut updated) = tokio::sync::watch::channel(project.clone());
        let producer = thread::spawn(move || {
            while !stopped.load(Ordering::Acquire) {
                if updated.has_changed().unwrap_or(false) {
                    project = updated.borrow_and_update().clone();
                    sources = pool.provider(project.clone(), services.clone());
                }
                if queue.capacity() - queue.len() >= 1024 {
                    fill(&project, sequence, &sources, &queue, &mut cursor, 1024);
                } else {
                    thread::sleep(Duration::from_millis(2));
                }
            }
        });
        let audio = Self { projects, stream: Some(stream), producer: Some(producer), stop, failed, clock, underruns, rate, origin };
        audio.stream.as_ref().ok_or_else(unavailable)?.play().map_err(|_| unavailable())?;
        Ok(audio)
    }
    pub fn time(&self) -> Tick {
        self.origin + Tick::from_units(self.clock.load(Ordering::Acquire) as i64, self.rate as i64)
    }
    pub fn update(&self, session: &Session) {
        let mut project = (*session.project).clone();
        if let Some(sequence) = session.state.active_sequence.and_then(|id| project.sequence_mut(id)) {
            sequence.settings.sample_rate = self.rate;
        }
        self.projects.send_replace(Arc::new(project));
    }
}
impl Drop for Audio {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.stream.take();
        if let Some(producer) = self.producer.take() {
            let _ = producer.join();
        }
        let missing = self.underruns.load(Ordering::Relaxed);
        if missing > 0 {
            eprintln!("audio_underrun: faltaram dados para {missing} amostras de áudio.");
        }
    }
}

fn unavailable() -> crate::protocol::Error {
    error("audio_unavailable", "Não foi possível iniciar o dispositivo de áudio padrão.")
}

fn fill(project: &Project, sequence: ItemId, sources: &dyn filmcraft_render::SourceProvider, queue: &ArrayQueue<[f32; 2]>, cursor: &mut i64, count: usize) {
    let Some(seq) = project.sequence(sequence) else {
        return;
    };
    let audio = filmcraft_render::audio::mix_sequence(project, seq, *cursor, count, sources);
    for i in 0..count {
        let left = audio.channels.first().and_then(|v| v.get(i)).copied().unwrap_or(0.0);
        let right = audio.channels.get(1).and_then(|v| v.get(i)).copied().unwrap_or(left);
        if queue.push([left, right]).is_err() {
            break;
        }
        *cursor += 1;
    }
}

struct Callback {
    queue: Arc<ArrayQueue<[f32; 2]>>,
    clock: Arc<AtomicU64>,
    failed: Arc<AtomicBool>,
    underruns: Arc<AtomicU64>,
    submitted: u64,
    rate: u32,
    channels: usize,
}
impl Callback {
    fn write<T: SizedSample + FromSample<f32>>(&mut self, data: &mut [T], info: &cpal::OutputCallbackInfo) {
        let stamp = info.timestamp();
        let latency = stamp.playback.duration_since(&stamp.callback).unwrap_or_default();
        let latency_samples = (latency.as_nanos() * self.rate as u128 / 1_000_000_000) as u64;
        // O início deste buffer só será audível depois da latência informada pelo dispositivo.
        // Publicamos o cursor já ouvido, nunca os samples apenas enfileirados.
        let audible = self.submitted.saturating_sub(latency_samples);
        self.clock.fetch_max(audible, Ordering::Release);
        for output in data.chunks_mut(self.channels) {
            let frame = self.queue.pop();
            if frame.is_some() {
                self.submitted += 1;
            } else {
                self.underruns.fetch_add(1, Ordering::Relaxed);
            }
            let [left, right] = frame.unwrap_or([0.0, 0.0]);
            for (channel, sample) in output.iter_mut().enumerate() {
                let value = match (self.channels, channel) {
                    (1, _) => (left + right) * 0.5,
                    (_, 0) => left,
                    (_, 1) => right,
                    _ => 0.0,
                };
                *sample = T::from_sample(value.clamp(-1.0, 1.0));
            }
        }
    }
}
fn build<T: SizedSample + FromSample<f32>>(device: &cpal::Device, config: &cpal::StreamConfig, mut input: Callback) -> Result<cpal::Stream> {
    let failed = input.failed.clone();
    device
        .build_output_stream(
            config,
            move |data, info| input.write::<T>(data, info),
            move |_| {
                failed.store(true, Ordering::Release);
            },
            None,
        )
        .map_err(|_| unavailable())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sample_clock_uses_exact_ticks() {
        let rate = 48_000;
        let origin = Tick::from_seconds_f64(12.5);
        let audio = Audio {
            projects: tokio::sync::watch::channel(Arc::new(Project::new("Teste"))).0,
            stream: None,
            producer: None,
            stop: Arc::new(AtomicBool::new(false)),
            failed: Arc::new(AtomicBool::new(false)),
            clock: Arc::new(AtomicU64::new(rate)),
            underruns: Arc::new(AtomicU64::new(0)),
            rate: rate as u32,
            origin,
        };
        assert_eq!(audio.time(), Tick::from_seconds_f64(13.5));
    }
}
