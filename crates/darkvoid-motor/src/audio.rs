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
    meter: tokio::sync::watch::Receiver<Option<serde_json::Value>>,
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
        let queue = Arc::new(ArrayQueue::new(rate as usize / 5));
        let mut cursor = origin.to_units_floor(rate as i64);
        fill(&project, sequence, &sources, &queue, &mut cursor, rate as usize / 5);
        let clock = Arc::new(AtomicU64::new(0));
        let failed = Arc::new(AtomicBool::new(false));
        let underruns = Arc::new(AtomicU64::new(0));
        let meter_queue = Arc::new(ArrayQueue::new(rate as usize / 2));
        let (meter_tx, meter) = tokio::sync::watch::channel(None);
        let input = Callback {
            queue: queue.clone(),
            clock: clock.clone(),
            failed: failed.clone(),
            underruns: underruns.clone(),
            meter_queue: meter_queue.clone(),
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
            let mut meter = crate::loudness::PlaybackMeter::new(rate);
            while !stopped.load(Ordering::Acquire) {
                while let Some(frame) = meter_queue.pop() {
                    if let Some(value) = meter.feed(frame) {
                        meter_tx.send_replace(Some(value));
                    }
                }
                if updated.has_changed().unwrap_or(false) && queue.capacity() - queue.len() >= rate as usize / 200 {
                    // A fila já entregue permanece contínua. Faz a troca no próximo
                    // trecho produzido, com 5 ms de interpolação entre snapshots.
                    let count = (queue.capacity() - queue.len()).min(rate as usize / 200);
                    let old = filmcraft_render::audio::mix_sequence(&project, project.sequence(sequence).expect("sequência de áudio"), cursor, count, &sources);
                    project = updated.borrow_and_update().clone();
                    sources = pool.provider(project.clone(), services.clone());
                    let new = filmcraft_render::audio::mix_sequence(&project, project.sequence(sequence).expect("sequência de áudio"), cursor, count, &sources);
                    for i in 0..count {
                        let weight = (i + 1) as f32 / count as f32;
                        let frame = [0, 1].map(|ch| old.channels[ch][i] * (1.0 - weight) + new.channels[ch][i] * weight);
                        if queue.push(frame).is_ok() {
                            cursor += 1;
                        }
                    }
                }
                if queue.capacity() - queue.len() >= 1024 {
                    fill(&project, sequence, &sources, &queue, &mut cursor, 1024);
                } else {
                    thread::sleep(Duration::from_millis(2));
                }
            }
        });
        let audio = Self { projects, stream: Some(stream), producer: Some(producer), stop, failed, clock, underruns, rate, origin, meter };
        audio.stream.as_ref().ok_or_else(unavailable)?.play().map_err(|_| unavailable())?;
        Ok(audio)
    }
    pub fn time(&self) -> Tick {
        self.origin + Tick::from_units(self.clock.load(Ordering::Acquire) as i64, self.rate as i64)
    }
    pub fn meter(&mut self) -> Option<serde_json::Value> {
        if self.meter.has_changed().unwrap_or(false) { self.meter.borrow_and_update().clone() } else { None }
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

pub(crate) fn fill(
    project: &Project,
    sequence: ItemId,
    sources: &dyn filmcraft_render::SourceProvider,
    queue: &ArrayQueue<[f32; 2]>,
    cursor: &mut i64,
    count: usize,
) {
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
    meter_queue: Arc<ArrayQueue<[f32; 2]>>,
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
            let _ = self.meter_queue.push([left.clamp(-1.0, 1.0), right.clamp(-1.0, 1.0)]);
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
    #[ignore = "Comparação M2.2: requer artefatos de audio_timeline_acceptance em MOTOR_M22_OUTPUT e FFmpeg."]
    fn preview_queue_matches_exported_mix() {
        let output = std::path::PathBuf::from(std::env::var("MOTOR_M22_OUTPUT").expect("MOTOR_M22_OUTPUT"));
        let mut session = Session::new(Arc::new(filmcraft_engine::FsServices));
        let mut clips = Vec::new();
        for i in 0..3 {
            clips.push(serde_json::json!({"id":format!("v{i}"),"media":output.join(format!("video-{i}.mp4")),"track":"V1","start":i as f64*2.0,"in":0.25,"out":2.25,"volume":1,"fadeIn":0,"fadeOut":0}));
        }
        clips
            .push(serde_json::json!({"id":"music","media":output.join("music.wav"),"track":"A1","start":0,"in":0,"out":6,"volume":0.3,"fadeIn":1,"fadeOut":1}));
        crate::timeline::Timeline::default().set(&mut session, serde_json::json!({"format":"horizontal","clips":clips})).unwrap();
        let sequence = session.state.active_sequence.unwrap();
        let sources = session.media.provider(session.project.clone(), session.services.clone());
        let queue = ArrayQueue::new(1024);
        let mut cursor = 0;
        let mut preview = Vec::new();
        while cursor < 6 * 48000 {
            let count = (6 * 48000 - cursor).min(1024) as usize;
            fill(&session.project, sequence, &sources, &queue, &mut cursor, count);
            while let Some(frame) = queue.pop() {
                preview.extend(frame);
            }
        }
        let decoded = std::process::Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-i"])
            .arg(output.join("timeline.mp4"))
            .args(["-vn", "-ac", "2", "-ar", "48000", "-f", "f32le", "pipe:1"])
            .output()
            .unwrap();
        assert!(decoded.status.success());
        let exported: Vec<_> = decoded.stdout.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect();
        assert!(exported.len() >= preview.len());
        let rmse = (preview.iter().zip(&exported).map(|(a, b)| f64::from(a - b).powi(2)).sum::<f64>() / preview.len() as f64).sqrt();
        assert!(rmse < 0.001, "RMSE prévia/exportação {rmse}");
        eprintln!("M2.2 fila da prévia versus AAC exportado: RMSE {rmse:.9}, {} amostras estéreo, atraso 0 amostras", preview.len() / 2);
        std::fs::write(output.join("preview-export.json"), serde_json::json!({"rmse":rmse,"stereoFrames":preview.len()/2,"lagSamples":0}).to_string()).unwrap();
    }
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
            meter: tokio::sync::watch::channel(None).1,
        };
        assert_eq!(audio.time(), Tick::from_seconds_f64(13.5));
    }
}
