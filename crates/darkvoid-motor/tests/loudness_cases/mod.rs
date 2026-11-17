use super::*;
use filmcraft_audio_dsp::LoudnessMeter;

fn audio(path: &std::path::Path) {
    let signal: Vec<_> = (0..480_000)
        .map(|i| {
            let t = i as f64 / 48000.0;
            (0.1 * (std::f64::consts::TAU * 997.0 * t).sin() + 0.02 * (std::f64::consts::TAU * 503.0 * t).sin()) as f32
        })
        .flat_map(f32::to_le_bytes)
        .collect();
    let bytes = signal.len() as u32;
    let mut wav = b"RIFF".to_vec();
    wav.extend((36 + bytes).to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16u32.to_le_bytes());
    wav.extend(3u16.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(48000u32.to_le_bytes());
    wav.extend(192000u32.to_le_bytes());
    wav.extend(4u16.to_le_bytes());
    wav.extend(32u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend(bytes.to_le_bytes());
    wav.extend(signal);
    std::fs::write(path, wav).unwrap();
}
fn clip(path: &std::path::Path, id: usize, gain: f64) -> Value {
    json!({"id":format!("c{id}"),"media":path,"track":"A1","start":id*10,"in":0,"out":10,
        "gain":gain,"volume":1,"fadeIn":0,"fadeOut":0})
}
async fn measured(socket: &mut Socket, id: u64, params: Value) -> Value {
    let started = request(socket, id, "audio.measure", params).await;
    assert_eq!(started["ok"], true, "{started}");
    let done = speech_job_finished(socket, &started["result"]["jobId"], "loudness", Duration::from_secs(1800)).await;
    assert_eq!(done["event"], "job.done", "{done}");
    done["data"]["result"].clone()
}

#[tokio::test]
async fn measure_gain_clip_snapshot_and_normalized_export() {
    let dir = tempfile::tempdir().unwrap();
    let wav = dir.path().join("sinal.wav");
    audio(&wav);
    let mut motor = Motor::start();
    let mut socket = motor.connect().await;
    assert_eq!(request(&mut socket, 1, "audio.measure", json!({})).await["error"]["code"], "no_session");
    let mut c = clip(&wav, 0, 0.0);
    let set = request(&mut socket, 2, "timeline.set", json!({"format":"horizontal","fps":1,"clips":[c.clone()]})).await;
    assert_eq!(set["ok"], true, "{set}");
    let baseline = measured(&mut socket, 3, json!({})).await;
    assert_eq!(baseline, measured(&mut socket, 4, json!({"clipId":"c0"})).await);
    assert_eq!(request(&mut socket, 5, "audio.measure", json!({"clipId":"missing"})).await["error"]["code"], "timeline_invalid");
    c["gain"] = json!(6.0);
    assert_eq!(request(&mut socket, 6, "timeline.set", json!({"format":"horizontal","fps":1,"clips":[c.clone()]})).await["ok"], true);
    let gained = measured(&mut socket, 7, json!({})).await;
    assert!((gained["integrated"].as_f64().unwrap() - baseline["integrated"].as_f64().unwrap() - 6.0).abs() < 0.01);
    c["gain"] = json!(24.1);
    assert_eq!(request(&mut socket, 8, "timeline.set", json!({"format":"horizontal","clips":[c]})).await["error"]["code"], "timeline_invalid");
    let target = dir.path().join("normalizado.mp4");
    let invalid =
        request(&mut socket, 9, "export.start", json!({"path":target,"preset":"youtube_1080p","encoder":"software","loudness":{"target":-14,"truePeak":1}}))
            .await;
    assert_eq!(invalid["error"]["code"], "invalid_params");
    let started =
        request(&mut socket, 10, "export.start", json!({"path":target,"preset":"youtube_1080p","encoder":"software","loudness":{"target":-14,"truePeak":-1}}))
            .await;
    assert_eq!(started["ok"], true, "{started}");
    let done = job_finished(&mut socket, &started["result"]["jobId"]).await;
    assert_eq!(done["event"], "job.done", "{done}");
    let session = filmcraft_engine::Session::new(std::sync::Arc::new(filmcraft_engine::FsServices));
    let decoded = session.media.probe_file(target.to_str().unwrap(), &*session.services).unwrap();
    let pcm = decoded.audio(0, 10 * 48000, 48000).unwrap();
    let mut meter = LoudnessMeter::new(48000.0, 2);
    meter.process(&pcm.channels.iter().map(Vec::as_slice).collect::<Vec<_>>());
    eprintln!("M4.1 MP4/AAC: integrado {:.6} LUFS; pico real {:.6} dBTP", meter.integrated(), meter.true_peak_dbtp());
    assert!((meter.integrated() + 14.0).abs() <= 0.5);
    assert!(meter.true_peak_dbtp() <= -1.0);
    request(&mut socket, 99, "shutdown", json!({})).await;
    motor.exited().await;
}

#[tokio::test]
#[ignore = "Aceite M4.1: mede toda uma linha do tempo sintética de 1 h; use --release --ignored --nocapture."]
async fn one_hour_measurement_and_cancellation() {
    let dir = tempfile::tempdir().unwrap();
    let wav = dir.path().join("sinal.wav");
    audio(&wav);
    let mut motor = Motor::start();
    let mut socket = motor.connect().await;
    let clips: Vec<_> = (0..360).map(|i| clip(&wav, i, 0.0)).collect();
    assert_eq!(request(&mut socket, 1, "timeline.set", json!({"format":"horizontal","clips":clips})).await["result"]["duration"], 3600.0);
    let start = Instant::now();
    let result = measured(&mut socket, 2, json!({})).await;
    eprintln!("M4.1 mistura 3600 s / 360 clipes: {:.3} s; {result}", start.elapsed().as_secs_f64());
    assert!(result["integrated"].as_f64().unwrap().is_finite());
    let job = request(&mut socket, 3, "audio.measure", json!({})).await;
    assert_eq!(request(&mut socket, 4, "audio.measure", json!({})).await["error"]["code"], "loudness_busy");
    assert_eq!(request(&mut socket, 5, "job.cancel", json!({"jobId":job["result"]["jobId"]})).await["ok"], true);
    let done = speech_job_finished(&mut socket, &job["result"]["jobId"], "loudness", Duration::from_secs(10)).await;
    assert_eq!(done["data"]["error"]["code"], "job_cancelled");
    request(&mut socket, 99, "shutdown", json!({})).await;
    motor.exited().await;
}

#[tokio::test]
#[ignore = "Aceite M4.1: reproduz sinal sintético baixo no dispositivo de áudio padrão por 4 s."]
async fn playback_emits_meter_at_most_ten_per_second() {
    let dir = tempfile::tempdir().unwrap();
    let wav = dir.path().join("sinal.wav");
    audio(&wav);
    let mut motor = Motor::start();
    let mut socket = motor.connect().await;
    assert_eq!(request(&mut socket, 1, "timeline.set", json!({"format":"horizontal","fps":1,"clips":[clip(&wav, 0, -12.0)]})).await["ok"], true);
    assert_eq!(request(&mut socket, 2, "transport.play", json!({})).await["ok"], true);
    let start = Instant::now();
    let mut times = Vec::new();
    let mut last = Value::Null;
    while start.elapsed() < Duration::from_secs(4) {
        let Message::Text(text) = next(&mut socket).await else { continue };
        let event: Value = serde_json::from_str(&text).unwrap();
        if event["event"] == "meter" {
            times.push(start.elapsed().as_secs_f64());
            last = event["data"].clone();
        }
    }
    assert!(times.len() >= 25 && times.len() <= 40, "{} eventos", times.len());
    for window in times.windows(2) {
        assert!(window[1] - window[0] >= 0.09, "{times:?}");
    }
    assert!(last["momentary"].as_f64().unwrap().is_finite());
    assert!(last["shortTerm"].as_f64().unwrap().is_finite());
    eprintln!("M4.1 reprodução física: {} eventos em {:.3} s; último {last}", times.len(), start.elapsed().as_secs_f64());
    request(&mut socket, 99, "shutdown", json!({})).await;
    motor.exited().await;
}
