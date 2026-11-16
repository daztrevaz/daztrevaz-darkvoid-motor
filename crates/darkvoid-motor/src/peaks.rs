//! Exact sample peaks, independent decoder, bounded worker and persistent cache.
use crate::protocol::{self, Result, error};
use filmcraft_engine::{FsServices, MediaPool};
use filmcraft_time::{TICKS_PER_SECOND, Tick};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::hash_map::DefaultHasher,
    fs::{self, File},
    hash::{Hash, Hasher},
    io::{self, BufWriter, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
};
use tokio::sync::oneshot;

const MAGIC: &[u8; 8] = b"DVPEAK01";
const HEADER: u64 = 40;
const BLOCK: usize = 256;
const MEMORY_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Clone)]
pub struct Media {
    pub path: PathBuf,
    pub duration: Tick,
}
#[derive(Deserialize)]
struct Params {
    buckets: usize,
}
struct Request {
    media: Input,
    buckets: usize,
    id: Value,
    reply: oneshot::Sender<Value>,
}

#[derive(Clone)]
pub enum Input {
    File(Media),
    Timeline(crate::mix::Snapshot),
}

pub struct Peaks {
    queue: Option<mpsc::SyncSender<Request>>,
    worker: Option<thread::JoinHandle<()>>,
    stop: Arc<AtomicBool>,
}
impl Peaks {
    pub fn new(directory: PathBuf) -> Self {
        let (tx, rx) = mpsc::sync_channel::<Request>(16);
        let stop = Arc::new(AtomicBool::new(false));
        let cancelled = stop.clone();
        let worker = thread::Builder::new()
            .name("darkvoid-peaks".into())
            .spawn(move || {
                let mut cached: Option<Envelope> = None;
                while let Ok(request) = rx.recv() {
                    if cancelled.load(Ordering::Acquire) {
                        break;
                    }
                    if request.reply.is_closed() {
                        continue;
                    }
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let media = match &request.media {
                            Input::Timeline(snapshot) => return timeline_peaks(snapshot, request.buckets, &cancelled),
                            Input::File(media) => media,
                        };
                        let key = fingerprint(media)?;
                        if cached.as_ref().is_none_or(|c| c.key != key) {
                            cached = None;
                            let path = directory.join(format!("peaks-v1-{key:016x}.bin"));
                            cached = Some(Envelope::load(&path, key, media.duration).or_else(|_| calculate(media, &path, key, &cancelled))?);
                        }
                        let envelope = cached.as_mut().ok_or_else(failed)?;
                        let peaks = envelope.buckets(request.buckets)?;
                        Ok(json!({"peaks":peaks}))
                    }))
                    .unwrap_or_else(|_| Err(failed()));
                    let _ = request.reply.send(protocol::response(request.id, result));
                }
            })
            .ok();
        Self { queue: worker.as_ref().map(|_| tx), worker, stop }
    }
    pub fn request(&self, media: Option<Media>, params: Value, id: Value, reply: oneshot::Sender<Value>) {
        self.request_input(media.map(Input::File).ok_or_else(|| error("no_session", "Abra um vídeo antes de calcular a onda de áudio.")), params, id, reply);
    }
    pub fn request_input(&self, media: Result<Input>, params: Value, id: Value, reply: oneshot::Sender<Value>) {
        let validated = (|| {
            let media = media?;
            let params: Params = protocol::field(params)?;
            if !(1..=20000).contains(&params.buckets) {
                return Err(protocol::invalid());
            }
            Ok((media, params.buckets))
        })();
        match validated {
            Ok((media, buckets)) => {
                let request = Request { media, buckets, id, reply };
                let Some(queue) = &self.queue else {
                    let _ = request.reply.send(protocol::response(request.id, Err(failed())));
                    return;
                };
                if let Err(e) = queue.try_send(request) {
                    let (request, message) = match e {
                        mpsc::TrySendError::Full(r) => (r, error("audio_peaks_busy", "Aguarde o cálculo da onda de áudio em andamento.")),
                        mpsc::TrySendError::Disconnected(r) => (r, failed()),
                    };
                    let _ = request.reply.send(protocol::response(request.id, Err(message)));
                }
            }
            Err(e) => {
                let _ = reply.send(protocol::response(id, Err(e)));
            }
        }
    }
}

fn timeline_peaks(snapshot: &crate::mix::Snapshot, buckets: usize, stop: &AtomicBool) -> Result<Value> {
    let rate = 48_000;
    let count = sample_count(snapshot.duration, rate);
    let read = snapshot.reader(rate);
    let mut peaks = vec![0.0f32; buckets];
    for start in (0..count).step_by(4096) {
        cancelled(stop)?;
        let channels = read(start as i64, (count - start).min(4096));
        for channel in channels {
            for (offset, sample) in channel.into_iter().enumerate() {
                let bucket = ((start + offset) as u128 * buckets as u128 * TICKS_PER_SECOND as u128 / (snapshot.duration.0 as u128 * rate as u128)) as usize;
                if let Some(peak) = peaks.get_mut(bucket) {
                    *peak = peak.max(sample.abs().min(1.0));
                }
            }
        }
    }
    Ok(json!({"peaks":peaks}))
}
impl Drop for Peaks {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.queue.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn failed() -> protocol::Error {
    error("audio_peaks_failed", "Não foi possível calcular a onda de áudio.")
}
fn io_error() -> protocol::Error {
    error("audio_peaks_io", "Não foi possível ler ou salvar o cache da onda de áudio.")
}
fn cancelled(stop: &AtomicBool) -> Result<()> {
    if stop.load(Ordering::Acquire) { Err(failed()) } else { Ok(()) }
}

/// Metadata identity: normal edits/replacements change size, modification or creation time.
fn fingerprint(media: &Media) -> Result<u64> {
    let path = fs::canonicalize(&media.path).map_err(|_| error("media_not_found", "Arquivo não encontrado."))?;
    let meta = fs::metadata(&path).map_err(|_| io_error())?;
    if !meta.is_file() {
        return Err(failed());
    }
    let mut key = DefaultHasher::new();
    (MAGIC, path, meta.len(), meta.modified().map_err(|_| io_error())?, meta.created().ok(), media.duration.0).hash(&mut key);
    Ok(key.finish())
}

struct Envelope {
    path: PathBuf,
    key: u64,
    duration: Tick,
    rate: u32,
    count: usize,
    blocks: Vec<f32>,
    samples: Option<Vec<f32>>,
    last: Option<(usize, Vec<f32>)>,
}
fn sample_count(duration: Tick, rate: u32) -> usize {
    (duration.0 as u128 * u128::from(rate)).div_ceil(TICKS_PER_SECOND as u128) as usize
}
fn boundary(bucket: usize, buckets: usize, duration: Tick, rate: u32) -> usize {
    (bucket as u128 * duration.0 as u128 * u128::from(rate)).div_ceil(buckets as u128 * TICKS_PER_SECOND as u128) as usize
}
fn valid(values: &[f32]) -> bool {
    values.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v))
}
fn write_values(out: &mut impl Write, values: &[f32]) -> io::Result<()> {
    let bytes: Vec<_> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
    out.write_all(&bytes)
}
fn read_values(input: &mut impl Read, count: usize) -> io::Result<Vec<f32>> {
    let mut bytes = vec![0; count * 4];
    input.read_exact(&mut bytes)?;
    let values: Vec<_> = bytes.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect();
    if !valid(&values) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid peaks"));
    }
    Ok(values)
}
impl Envelope {
    fn load(path: &Path, key: u64, duration: Tick) -> Result<Self> {
        let mut file = File::open(path).map_err(|_| io_error())?;
        let mut header = [0; 40];
        file.read_exact(&mut header).map_err(|_| io_error())?;
        let number = |start: usize| {
            let mut b = [0; 8];
            b.copy_from_slice(&header[start..start + 8]);
            u64::from_le_bytes(b)
        };
        let rate = u32::from_le_bytes([header[24], header[25], header[26], header[27]]);
        let count = number(28);
        if &header[..8] != MAGIC
            || number(8) != key
            || number(16) != duration.0 as u64
            || rate > filmcraft_media::MAX_AUDIO_SAMPLE_RATE
            || duration <= Tick::ZERO
            || duration > Tick::from_seconds_f64(3600.0)
            || count != sample_count(duration, rate) as u64
        {
            return Err(io_error());
        }
        let blocks = (count as usize).div_ceil(BLOCK);
        if file.metadata().map_err(|_| io_error())?.len() != HEADER + count * 4 + blocks as u64 * 4 {
            return Err(io_error());
        }
        let samples = if count * 4 <= MEMORY_BYTES {
            Some(read_values(&mut file, count as usize).map_err(|_| io_error())?)
        } else {
            file.seek(SeekFrom::Start(HEADER + count * 4)).map_err(|_| io_error())?;
            None
        };
        let blocks = read_values(&mut file, blocks).map_err(|_| io_error())?;
        Ok(Self { path: path.into(), key, duration, rate, count: count as usize, blocks, samples, last: None })
    }
    fn buckets(&mut self, buckets: usize) -> Result<Vec<f32>> {
        if let Some((n, peaks)) = &self.last
            && *n == buckets
        {
            return Ok(peaks.clone());
        }
        if self.rate == 0 {
            return Ok(vec![0.0; buckets]);
        }
        let mut file = if self.samples.is_none() { Some(File::open(&self.path).map_err(|_| io_error())?) } else { None };
        let mut result = Vec::with_capacity(buckets);
        for bucket in 0..buckets {
            let start = boundary(bucket, buckets, self.duration, self.rate).min(self.count);
            let end = boundary(bucket + 1, buckets, self.duration, self.rate).min(self.count);
            let first = start.div_ceil(BLOCK).min(end / BLOCK);
            let last = end / BLOCK;
            let mut peak = 0.0f32;
            if first < last {
                peak = self.blocks[first..last].iter().copied().fold(peak, f32::max);
                peak = self.edge(start, first * BLOCK, &mut file)?.max(peak);
                peak = self.edge(last * BLOCK, end, &mut file)?.max(peak);
            } else {
                peak = self.edge(start, end, &mut file)?;
            }
            result.push(peak);
        }
        self.last = Some((buckets, result.clone()));
        Ok(result)
    }
    fn edge(&self, start: usize, end: usize, file: &mut Option<File>) -> Result<f32> {
        if let Some(samples) = &self.samples {
            return Ok(samples[start..end].iter().copied().fold(0.0, f32::max));
        }
        let file = file.as_mut().ok_or_else(io_error)?;
        file.seek(SeekFrom::Start(HEADER + start as u64 * 4)).map_err(|_| io_error())?;
        Ok(read_values(file, end - start).map_err(|_| io_error())?.into_iter().fold(0.0, f32::max))
    }
}

fn calculate(media: &Media, path: &Path, key: u64, stop: &Arc<AtomicBool>) -> Result<Envelope> {
    cancelled(stop)?;
    let source = MediaPool::default().probe_file(&media.path.to_string_lossy(), &FsServices).map_err(|_| failed())?;
    let rate = source.info().audio_streams.iter().map(|a| a.sample_rate).max().unwrap_or(0);
    if rate > filmcraft_media::MAX_AUDIO_SAMPLE_RATE {
        return Err(failed());
    }
    let count = sample_count(media.duration, rate);
    let mut temp = tempfile::NamedTempFile::new_in(path.parent().ok_or_else(io_error)?).map_err(|_| io_error())?;
    let mut samples = (count as u64 * 4 <= MEMORY_BYTES).then(|| Vec::with_capacity(count));
    let mut blocks = Vec::with_capacity(count.div_ceil(BLOCK));
    let mut block_peak = 0.0f32;
    {
        let mut out = BufWriter::new(temp.as_file_mut());
        out.write_all(MAGIC).map_err(|_| io_error())?;
        out.write_all(&key.to_le_bytes()).map_err(|_| io_error())?;
        out.write_all(&media.duration.0.to_le_bytes()).map_err(|_| io_error())?;
        out.write_all(&rate.to_le_bytes()).map_err(|_| io_error())?;
        out.write_all(&(count as u64).to_le_bytes()).map_err(|_| io_error())?;
        out.write_all(&[0; 4]).map_err(|_| io_error())?;
        let mut start = 0;
        while start < count {
            cancelled(stop)?;
            let frames = (count - start).min(rate as usize);
            let mut peaks = vec![0.0f32; frames];
            for stream in 0..source.info().audio_streams.len() {
                let audio = filmcraft_media::cancel::with_cancel(stop, || source.audio_stream(stream, start as i64, frames, rate)).map_err(|_| failed())?;
                if audio.channels.is_empty() || audio.channels.iter().any(|c| c.len() != frames) {
                    return Err(failed());
                }
                for channel in &audio.channels {
                    for (peak, value) in peaks.iter_mut().zip(channel) {
                        if !value.is_finite() {
                            return Err(failed());
                        }
                        *peak = peak.max(value.abs().min(1.0));
                    }
                }
            }
            for (i, &peak) in peaks.iter().enumerate() {
                block_peak = block_peak.max(peak);
                if (start + i + 1) % BLOCK == 0 {
                    blocks.push(block_peak);
                    block_peak = 0.0;
                }
            }
            write_values(&mut out, &peaks).map_err(|_| io_error())?;
            if let Some(samples) = &mut samples {
                samples.extend_from_slice(&peaks);
            }
            start += frames;
        }
        if !count.is_multiple_of(BLOCK) {
            blocks.push(block_peak);
        }
        write_values(&mut out, &blocks).map_err(|_| io_error())?;
        out.flush().map_err(|_| io_error())?;
    }
    cancelled(stop)?;
    if fingerprint(media)? != key {
        return Err(error("media_changed", "O arquivo mudou durante o cálculo. Abra o vídeo novamente."));
    }
    temp.as_file().sync_all().map_err(|_| io_error())?;
    temp.persist(path).map_err(|_| io_error())?;
    Ok(Envelope { path: path.into(), key, duration: media.duration, rate, count, blocks, samples, last: None })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn wav(path: &Path, frames: &[[f32; 2]], rate: u32) {
        let data: Vec<u8> = frames.iter().flatten().flat_map(|v| v.to_le_bytes()).collect();
        let mut bytes = b"RIFF".to_vec();
        bytes.extend((36 + data.len() as u32).to_le_bytes());
        bytes.extend(b"WAVEfmt ");
        bytes.extend(16u32.to_le_bytes());
        bytes.extend(3u16.to_le_bytes());
        bytes.extend(2u16.to_le_bytes());
        bytes.extend(rate.to_le_bytes());
        bytes.extend((rate * 8).to_le_bytes());
        bytes.extend(8u16.to_le_bytes());
        bytes.extend(32u16.to_le_bytes());
        bytes.extend(b"data");
        bytes.extend((data.len() as u32).to_le_bytes());
        bytes.extend(data);
        fs::write(path, bytes).unwrap();
    }
    #[test]
    fn exact_buckets_keep_impulses_channels_and_fractional_boundaries_after_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source.wav");
        let mut frames = vec![[0.0, 0.0]; 1001];
        for (i, f) in frames.iter_mut().enumerate() {
            *f = [(i % 19) as f32 / 20.0, -((i % 23) as f32 / 24.0)];
        }
        frames[255] = [1.5, -1.5];
        frames[511] = [0.8, -0.8];
        frames[1000] = [-0.999, 0.999];
        wav(&path, &frames, 8000);
        let media = Media { path, duration: Tick::from_units(frames.len() as i64, 8000) };
        let key = fingerprint(&media).unwrap();
        let cache = dir.path().join("peaks.bin");
        let mut first = calculate(&media, &cache, key, &Arc::new(AtomicBool::new(false))).unwrap();
        let mut loaded = Envelope::load(&cache, key, media.duration).unwrap();
        let mut disk = Envelope::load(&cache, key, media.duration).unwrap();
        disk.samples = None;
        for buckets in [1, 2, 3, 7, 256, 999, 1001, 20000] {
            // Independent oracle: assign each sample's timestamp to its slice, not query boundaries.
            let mut expected = vec![0.0f32; buckets];
            for (index, frame) in frames.iter().enumerate() {
                let bucket = (index as u128 * buckets as u128 * TICKS_PER_SECOND as u128 / (media.duration.0 as u128 * 8000)) as usize;
                if let Some(peak) = expected.get_mut(bucket) {
                    *peak = frame.iter().map(|v| v.abs().min(1.0)).fold(*peak, f32::max);
                }
            }
            assert_eq!(first.buckets(buckets).unwrap(), expected);
            assert_eq!(loaded.buckets(buckets).unwrap(), expected);
            assert_eq!(disk.buckets(buckets).unwrap(), expected);
        }
        assert!(Envelope::load(&cache, key + 1, media.duration).is_err());
        fs::write(&cache, b"partial").unwrap();
        assert!(Envelope::load(&cache, key, media.duration).is_err());
        assert!(calculate(&media, &cache, key, &Arc::new(AtomicBool::new(true))).is_err());
    }
    #[test]
    fn identity_changes_on_replacement_size_and_write_time() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source.wav");
        fs::write(&path, b"one").unwrap();
        let media = Media { path, duration: Tick::from_seconds_f64(1.0) };
        let first = fingerprint(&media).unwrap();
        fs::write(&media.path, b"two").unwrap();
        File::options().write(true).open(&media.path).unwrap().set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(2)).unwrap();
        assert_ne!(first, fingerprint(&media).unwrap());
        let second = fingerprint(&media).unwrap();
        fs::write(&media.path, b"larger").unwrap();
        assert_ne!(second, fingerprint(&media).unwrap());
    }
    #[tokio::test]
    async fn requests_share_disk_cache_across_bucket_counts_and_process_workers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source.wav");
        wav(&path, &[[0.8, -0.8]; 8000], 8000);
        let media = Media { path, duration: Tick::from_seconds_f64(1.0) };
        let mut worker = Peaks::new(dir.path().into());
        for buckets in [3, 7, 20000] {
            let (tx, rx) = oneshot::channel();
            worker.request(Some(media.clone()), json!({"buckets":buckets}), json!(buckets), tx);
            let response = rx.await.unwrap();
            assert_eq!(response["ok"], true, "{response}");
            assert_eq!(response["result"]["peaks"].as_array().unwrap().len(), buckets);
        }
        let cached: Vec<_> = fs::read_dir(dir.path()).unwrap().flatten().filter(|p| p.path().extension().is_some_and(|e| e == "bin")).collect();
        assert_eq!(cached.len(), 1);
        let written = cached[0].metadata().unwrap().modified().unwrap();
        drop(worker);
        worker = Peaks::new(dir.path().into());
        let (tx, rx) = oneshot::channel();
        worker.request(Some(media), json!({"buckets":1}), json!(1), tx);
        assert_eq!(rx.await.unwrap()["result"]["peaks"][0], json!(0.8f32));
        assert_eq!(cached[0].metadata().unwrap().modified().unwrap(), written);
    }
}
