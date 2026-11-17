//! Medição em blocos limitados, fora da thread de comandos e do callback de áudio.
use crate::{
    mix::Snapshot,
    protocol::{Result, error},
};
use filmcraft_audio_dsp::LoudnessMeter;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
use tokio::sync::broadcast;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Measure {
    pub clip_id: Option<String>,
}

pub fn summary(meter: &LoudnessMeter) -> Value {
    // JSON não representa infinito: silêncio e janelas ainda incompletas usam null.
    json!({"integrated":meter.integrated(),"range":meter.loudness_range(),"truePeak":meter.true_peak_dbtp(),
        "maxMomentary":meter.max_momentary(),"maxShortTerm":meter.max_short_term()})
}

pub fn measure(snapshot: Snapshot, cancel: &Arc<AtomicBool>, progress: &AtomicU64) -> Result<Value> {
    let rate = 48_000;
    let total = snapshot.duration.to_units_floor(rate);
    let read = snapshot.reader(rate as u32);
    let mut meter = LoudnessMeter::new(rate as f64, 2);
    let mut cursor = 0;
    while cursor < total {
        if cancel.load(Ordering::Acquire) {
            return Err(error("job_cancelled", "Medição de loudness cancelada."));
        }
        let count = (total - cursor).min(rate) as usize;
        let channels = filmcraft_media::cancel::with_cancel(cancel, || read(cursor, count));
        if channels.len() != 2 || channels.iter().any(|c| c.len() != count || c.iter().any(|v| !v.is_finite())) {
            return Err(error("audio_decode_failed", "O áudio decodificado está incompleto ou inválido."));
        }
        meter.process(&channels.iter().map(Vec::as_slice).collect::<Vec<_>>());
        cursor += count as i64;
        progress.store((cursor as f64 / total.max(1) as f64 * 1_000_000.0) as u64, Ordering::Relaxed);
    }
    if cancel.load(Ordering::Acquire) {
        return Err(error("job_cancelled", "Medição de loudness cancelada."));
    }
    Ok(summary(&meter))
}

struct Job {
    id: u64,
    cancel: Arc<AtomicBool>,
    progress: Arc<AtomicU64>,
    worker: thread::JoinHandle<()>,
    result: mpsc::Receiver<Result<Value>>,
    emitted: Instant,
}
pub struct Loudness {
    ids: Arc<AtomicU64>,
    events: broadcast::Sender<Value>,
    active: Option<Job>,
}
impl Loudness {
    pub fn new(ids: Arc<AtomicU64>, events: broadcast::Sender<Value>) -> Self {
        Self { ids, events, active: None }
    }
    pub fn start(&mut self, snapshot: Snapshot) -> Result<Value> {
        if self.active.is_some() {
            return Err(error("loudness_busy", "Aguarde ou cancele a medição de loudness em andamento."));
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(AtomicU64::new(0));
        let stopped = cancel.clone();
        let observed = progress.clone();
        let (tx, result) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("darkvoid-loudness".into())
            .spawn(move || {
                let value = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| measure(snapshot, &stopped, &observed)))
                    .unwrap_or_else(|_| Err(error("loudness_failed", "Falha na medição de loudness.")));
                let _ = tx.send(value);
            })
            .map_err(|_| error("loudness_failed", "Não foi possível iniciar a medição de loudness."))?;
        let id = self.ids.fetch_add(1, Ordering::Relaxed);
        self.active = Some(Job { id, cancel, progress, worker, result, emitted: Instant::now() });
        Ok(json!({"jobId":id}))
    }
    pub fn cancel(&self, id: u64) -> bool {
        if let Some(job) = &self.active
            && job.id == id
        {
            job.cancel.store(true, Ordering::Release);
            true
        } else {
            false
        }
    }
    pub fn poll(&mut self) {
        let Some(job) = &mut self.active else { return };
        match job.result.try_recv() {
            Err(mpsc::TryRecvError::Empty) => {
                if job.emitted.elapsed() >= Duration::from_millis(250) {
                    let _ = self.events.send(json!({"event":"job.progress","data":{"jobId":job.id,"kind":"loudness",
                        "fraction":job.progress.load(Ordering::Relaxed) as f64 / 1_000_000.0,"etaSeconds":null}}));
                    job.emitted = Instant::now();
                }
                return;
            }
            outcome => {
                let outcome = outcome.unwrap_or_else(|_| Err(error("loudness_failed", "Medição de loudness interrompida.")));
                let event = match outcome {
                    Ok(result) => json!({"event":"job.done","data":{"jobId":job.id,"kind":"loudness","result":result}}),
                    Err(e) => json!({"event":"job.error","data":{"jobId":job.id,"kind":"loudness","error":{"code":e.code,"message":e.message}}}),
                };
                let _ = self.events.send(event);
            }
        }
        if let Some(job) = self.active.take() {
            let _ = job.worker.join();
        }
    }
}
impl Drop for Loudness {
    fn drop(&mut self) {
        if let Some(job) = self.active.take() {
            job.cancel.store(true, Ordering::Release);
            let _ = job.worker.join();
        }
    }
}

/// Amostras efetivamente consumidas pelo dispositivo; o produtor faz o DSP.
pub struct PlaybackMeter {
    meter: LoudnessMeter,
    pending: usize,
    interval: usize,
}
impl PlaybackMeter {
    pub fn new(rate: u32) -> Self {
        Self { meter: LoudnessMeter::new(rate as f64, 2), pending: 0, interval: (rate as usize / 10).max(1) }
    }
    pub fn feed(&mut self, frame: [f32; 2]) -> Option<Value> {
        self.meter.process_interleaved(&frame);
        self.pending += 1;
        if self.pending < self.interval {
            return None;
        }
        self.pending = 0;
        Some(json!({"event":"meter","data":{"momentary":self.meter.momentary(),"shortTerm":self.meter.short_term(),
            "truePeakL":20.0*self.meter.true_peak_channel(0).log10(),"truePeakR":20.0*self.meter.true_peak_channel(1).log10()}}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn playback_meter_caps_rate_and_serializes_silence() {
        let mut meter = PlaybackMeter::new(48000);
        let mut events = 0;
        for _ in 0..48000 {
            if let Some(event) = meter.feed([0.0, 0.0]) {
                events += 1;
                assert!(event["data"]["momentary"].is_null());
                assert!(event["data"]["truePeakL"].is_null());
            }
        }
        assert_eq!(events, 10);
        assert!(summary(&LoudnessMeter::new(48000.0, 2))["integrated"].is_null());
    }
    #[test]
    fn measurement_matches_engine_meter_and_clip_isolation() {
        let mut session = filmcraft_engine::Session::new(Arc::new(filmcraft_engine::FsServices));
        let mut timeline = crate::timeline::Timeline::default();
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/padrao.mp4");
        let clips = json!([
            {"id":"a","media":path,"track":"V1","start":1,"in":0,"out":2,"gain":-6,"volume":1,"fadeIn":0,"fadeOut":0},
            {"id":"b","media":path,"track":"V2","start":0,"in":0,"out":2,"volume":1,"fadeIn":0,"fadeOut":0}
        ]);
        timeline.set(&mut session, json!({"format":"horizontal","clips":clips})).unwrap();
        let snapshot = Snapshot::new(&session).unwrap();
        let signal = snapshot.reader(48000)(0, 3 * 48000);
        let mut oracle = LoudnessMeter::new(48000.0, 2);
        // Oráculo do engine alimentado em blocos de 100 ms, como loudness_oracle.
        for start in (0..3 * 48000).step_by(4800) {
            oracle.process(&signal.iter().map(|c| &c[start..start + 4800]).collect::<Vec<_>>());
        }
        let actual = measure(snapshot, &Arc::new(AtomicBool::new(false)), &AtomicU64::new(0)).unwrap();
        assert_eq!(actual, summary(&oracle));
        let (ids, start, duration) = timeline.clip_audio("a").unwrap();
        let isolated = Snapshot::clip(&session, &ids, start, duration).unwrap();
        assert_eq!(isolated.duration.seconds(), 2.0);
        let clip = measure(isolated, &Arc::new(AtomicBool::new(false)), &AtomicU64::new(0)).unwrap();
        assert!(clip["integrated"].as_f64().unwrap() < actual["integrated"].as_f64().unwrap());
    }
}
