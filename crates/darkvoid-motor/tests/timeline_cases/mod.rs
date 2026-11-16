use super::*;

fn clip(id: &str, media: &str, track: &str, start: f64, source_in: f64, out: f64) -> Value {
    json!({"id":id,"media":media,"track":track,"start":start,"in":source_in,"out":out,"volume":1,"fadeIn":0,"fadeOut":0})
}
fn timeline(clips: Vec<Value>) -> Value {
    json!({"format":"horizontal","fps":30,"clips":clips})
}

#[tokio::test]
async fn timeline_probe_images_audio_preserves_config_playhead_and_captions() {
    let mut motor = Motor::start();
    let mut socket = motor.connect().await;
    for ext in ["png", "jpg", "webp"] {
        let path = motor._dir.path().join(format!("imagem.{ext}"));
        image::RgbImage::from_pixel(32, 18, image::Rgb([240u8, 10, 20])).save(&path).unwrap();
        let result = request(&mut socket, 1, "media.probe", json!({"path":path})).await;
        assert_eq!(result["ok"], true, "{result}");
        assert_eq!(result["result"]["kind"], "image");
        assert_eq!(result["result"]["duration"], 0.0);
        assert_eq!(result["result"]["fps"], 0.0);
        assert_eq!(result["result"]["hasAudio"], false);
        assert_eq!(result["result"]["width"], 32);
        assert_eq!(result["result"]["videoCodec"], ext.to_ascii_uppercase());
    }
    let path = motor._dir.path().join("tom.wav");
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&96036u32.to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&48000u32.to_le_bytes());
    wav.extend_from_slice(&96000u32.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&96000u32.to_le_bytes());
    wav.resize(96044, 0);
    std::fs::write(&path, wav).unwrap();
    let result = request(&mut socket, 2, "media.probe", json!({"path":path})).await;
    assert_eq!(result["result"]["kind"], "audio", "{result}");
    for field in ["width", "height", "fps"] {
        assert_eq!(result["result"][field].as_f64(), Some(0.0));
    }
    assert!(result["result"]["videoCodec"].is_null());
    let bad_audio = timeline(vec![clip("audio-invalido", path.to_str().unwrap(), "V1", 0.0, 0.0, 1.0)]);
    let result = request(&mut socket, 41, "timeline.set", bad_audio).await;
    assert_eq!(result["error"]["code"], "timeline_invalid");
    assert!(result["error"]["message"].as_str().unwrap().contains("audio-invalido"));
    let p = timeline(vec![clip("a", &fixture(), "V1", 0.0, 0.0, 3.0)]);
    assert_eq!(request(&mut socket, 3, "preview.config", json!({"maxWidth":320,"maxHeight":180,"quality":85})).await["ok"], true);
    assert_eq!(request(&mut socket, 4, "timeline.set", p.clone()).await["ok"], true);
    assert_eq!(frame(&mut socket, 0.0).await, (320, 180));
    let peaks = request(&mut socket, 40, "audio.peaks", json!({"buckets":30})).await;
    assert_eq!(peaks["ok"], true, "{peaks}");
    let speech = request(&mut socket, 40, "speech.transcribe", json!({"model":"whisper-small","language":"pt"})).await;
    assert_eq!(speech["error"]["code"], "model_not_installed");
    assert_eq!(request(&mut socket, 5, "transport.seek", json!({"seconds":1.0})).await["ok"], true);
    frame(&mut socket, 1.0).await;
    command_frame(&mut socket, 6, "captions.set", caption_cases::captions("phrase", 3)).await;
    let jpeg = command_frame(&mut socket, 7, "timeline.set", p.clone()).await;
    assert_eq!(image::load_from_memory(&jpeg).unwrap().width(), 320);
    let state = request(&mut socket, 8, "transport.pause", json!({})).await;
    assert_eq!(state["ok"], true);
    // Redesenho no mesmo tempo e legendas preservadas após atualização parcial.
    let mut shorter = p.clone();
    shorter["clips"][0]["out"] = json!(0.5);
    assert_eq!(request(&mut socket, 9, "timeline.set", shorter).await["ok"], true);
    frame(&mut socket, 0.5).await;
    let srt = motor._dir.path().join("timeline.srt");
    assert_eq!(request(&mut socket, 10, "captions.exportSrt", json!({"path":srt})).await["ok"], true);
    assert!(std::fs::read_to_string(srt).unwrap().contains("Olá"));
    let mut reset = p;
    reset["format"] = json!("vertical");
    assert_eq!(request(&mut socket, 11, "timeline.set", reset).await["ok"], true);
    assert_eq!(frame(&mut socket, 0.0).await, (101, 180));
    request(&mut socket, 99, "shutdown", json!({})).await;
    motor.exited().await;
}

#[tokio::test]
#[ignore = "Aceite M2.1: MOTOR_BENCH_MEDIA sintético 1080p de pelo menos 12 s; áudio físico e FFmpeg."]
async fn timeline_benchmark_300_clips_one_hour() {
    let media = std::env::var("MOTOR_BENCH_MEDIA").unwrap();
    let output = std::path::PathBuf::from(std::env::var("MOTOR_M21_OUTPUT").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    let mut motor = Motor::start();
    let mut socket = motor.connect().await;
    let hello = request(&mut socket, 1, "hello", json!({})).await;
    let clips: Vec<_> = (0..300).map(|i| clip(&format!("c{i}"), &media, "V1", f64::from(i * 12), 0.0, 12.0)).collect();
    let mut params = timeline(clips);
    let start = Instant::now();
    let first = request(&mut socket, 2, "timeline.set", params.clone()).await;
    let first_ms = start.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(first["ok"], true, "{first}");
    assert_eq!(first["result"]["duration"], 3600.0);
    frame(&mut socket, 0.0).await;
    let mut update_ms = Vec::new();
    for i in 0..10 {
        params["clips"][150]["muted"] = json!(i % 2 == 0);
        let start = Instant::now();
        assert_eq!(request(&mut socket, 3, "timeline.set", params.clone()).await["ok"], true);
        update_ms.push(start.elapsed().as_secs_f64() * 1000.0);
        frame(&mut socket, 0.0).await;
    }
    // Atravessa dois cortes de mídia não contígua na mesma fonte.
    command_frame(&mut socket, 4, "transport.seek", json!({"seconds":11.0})).await;
    assert_eq!(request(&mut socket, 5, "transport.play", json!({})).await["ok"], true);
    let start = Instant::now();
    let mut frames = Vec::new();
    let mut stats = Vec::new();
    let mut times = Vec::new();
    while start.elapsed() < Duration::from_secs(15) {
        match next(&mut socket).await {
            Message::Binary(bytes) => {
                assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 1920);
                assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), 1080);
                frames.push(Instant::now());
                times.push(f64::from_le_bytes(bytes[16..24].try_into().unwrap()));
            }
            Message::Text(text) => {
                let v: Value = serde_json::from_str(&text).unwrap();
                if v["event"] == "stats" {
                    stats.push(v["data"].clone());
                }
            }
            _ => {}
        }
    }
    params["clips"][150]["muted"] = json!(true);
    let updating = Instant::now();
    assert_eq!(request(&mut socket, 30, "timeline.set", params).await["ok"], true);
    let playing_update_ms = updating.elapsed().as_secs_f64() * 1000.0;
    request(&mut socket, 6, "transport.pause", json!({})).await;
    let fps = (frames.len() - 1) as f64 / frames.last().unwrap().duration_since(frames[0]).as_secs_f64();
    let max_gap = frames.windows(2).map(|w| w[1].duration_since(w[0]).as_secs_f64() * 1000.0).fold(0.0, f64::max);
    let cut_gaps: Vec<_> = times
        .windows(2)
        .zip(frames.windows(2))
        .filter(|(t, _)| [12.0, 24.0].iter().any(|cut| t[0] < *cut && t[1] >= *cut))
        .map(|(t, w)| json!({"from":t[0],"to":t[1],"gapMs":w[1].duration_since(w[0]).as_secs_f64()*1000.0}))
        .collect();
    assert!(times.iter().any(|t| *t > 24.0));
    let mut seeks = Vec::new();
    for time in [1800.5, 3599.0, 12.0, 24.0, 0.5] {
        let start = Instant::now();
        command_frame(&mut socket, 7, "transport.seek", json!({"seconds":time})).await;
        seeks.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    // Exporta uma amostra curta com vazio, V1, V2, imagem e legendas; compara todo quadro por JPEG.
    let image = output.join("image.png");
    image::RgbImage::from_pixel(640, 360, image::Rgb([20u8, 40, 220])).save(&image).unwrap();
    let params = timeline(vec![clip("v1", &media, "V1", 0.2, 0.0, 0.8), clip("v2", image.to_str().unwrap(), "V2", 0.5, 0.0, 0.3)]);
    command_frame(&mut socket, 8, "timeline.set", params).await;
    let mut captions = caption_cases::captions("phrase", 3);
    captions["cues"] = json!([{"start":0.2,"end":0.9,"words":[{"text":"DARKVOID","start":0.2,"end":0.9}]}]);
    command_frame(&mut socket, 9, "captions.set", captions).await;
    let encoder = if hello["result"]["encoders"].as_array().unwrap().iter().any(|e| e == "nvenc_h264") { "nvenc_h264" } else { "software" };
    let path = output.join("timeline.mp4");
    let start = Instant::now();
    let job = request(&mut socket, 10, "export.start", json!({"path":path,"preset":"youtube_1080p","encoder":encoder,"burnCaptions":true})).await;
    assert_eq!(job["ok"], true, "{job}");
    let done = job_finished(&mut socket, &job["result"]["jobId"]).await;
    assert_eq!(done["event"], "job.done", "{done}");
    let export_s = start.elapsed().as_secs_f64();
    let mut comparisons = Vec::new();
    for time in [0.0, 0.3, 0.6, 0.9] {
        let jpeg = command_frame(&mut socket, 11, "transport.seek", json!({"seconds":time})).await;
        let preview = image::load_from_memory(&jpeg).unwrap().into_rgb8();
        let decoded = Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-ss", &format!("{time:.9}"), "-i"])
            .arg(&path)
            .args(["-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgb24", "pipe:1"])
            .output()
            .unwrap();
        assert!(decoded.status.success());
        assert_eq!(preview.as_raw().len(), decoded.stdout.len());
        let mae = preview.as_raw().iter().zip(&decoded.stdout).map(|(a, b)| u64::from(a.abs_diff(*b))).sum::<u64>() as f64 / decoded.stdout.len() as f64;
        comparisons.push(json!({"seconds":time,"mae":mae}));
        std::fs::write(output.join(format!("preview-{time}.jpg")), jpeg).unwrap();
    }
    let mut audio_probes = Vec::new();
    for (ext, codec) in [("wav", "pcm_s16le"), ("mp3", "libmp3lame"), ("m4a", "aac"), ("aac", "aac"), ("flac", "flac")] {
        let path = output.join(format!("probe.{ext}"));
        let status =
            Command::new("ffmpeg").args(["-hide_banner", "-loglevel", "error", "-i", &media, "-t", "0.25", "-vn", "-c:a", codec]).arg(&path).status().unwrap();
        assert!(status.success());
        let info = request(&mut socket, 20, "media.probe", json!({"path":path})).await;
        assert_eq!(info["ok"], true, "{ext}: {info}");
        assert_eq!(info["result"]["kind"], "audio", "{ext}: {info}");
        audio_probes.push(json!({"format":ext,"result":info["result"]}));
    }
    let report = json!({"gpu":hello["result"]["gpu"],"firstSetMs":first_ms,"partialSetMs":update_ms,"playingUpdateMs":playing_update_ms,"fps":fps,"maxFrameGapMs":max_gap,"cutGaps":cut_gaps,"seekMs":seeks,"stats":stats,"exportSeconds":export_s,"exportComparisons":comparisons,"audioProbes":audio_probes});
    eprintln!("M2.1 {report}");
    std::fs::write(output.join("measurements.json"), serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    request(&mut socket, 99, "shutdown", json!({})).await;
    motor.exited().await;
    assert!(first_ms < 50.0 && update_ms.iter().all(|n| *n < 50.0), "timeline.set acima de 50 ms");
    assert!(playing_update_ms < 50.0, "timeline.set durante play acima de 50 ms");
    assert!(fps >= 29.5, "prévia abaixo de 30 qps");
    assert!(seeks.iter().all(|n| *n < 300.0), "seek acima de 300 ms");
    assert!(comparisons.iter().all(|c| c["mae"].as_f64().unwrap() < 8.0), "exportação diverge da prévia");
}
