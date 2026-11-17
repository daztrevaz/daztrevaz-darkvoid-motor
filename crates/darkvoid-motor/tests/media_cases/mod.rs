use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};

#[tokio::test]
async fn shutdown_cancels_thumbnails_and_peaks_together() {
    let mut motor = Motor::start();
    let mut socket = motor.connect().await;
    request(&mut socket, 1, "hello", json!({})).await;
    for (id, cmd, params) in
        [(2, "media.thumbs", json!({"media":fixture(),"count":200,"height":240})), (3, "audio.peaks", json!({"media":fixture(),"buckets":20000}))]
    {
        socket.send(Message::Text(json!({"id":id,"cmd":cmd,"params":params}).to_string().into())).await.unwrap();
    }
    assert_eq!(request(&mut socket, 99, "shutdown", json!({})).await["ok"], true);
    motor.exited().await;
}

#[tokio::test]
async fn thumbs_validate_decode_cache_and_survive_restart() {
    let data = tempfile::tempdir().unwrap();
    let mut motor = Motor::start_with_data_dir(Some(data.path()));
    let mut socket = motor.connect().await;
    assert_eq!(request(&mut socket, 1, "hello", json!({})).await["result"]["protocol"], 4);
    for (count, height) in [
        (json!(0), json!(120)),
        (json!(201), json!(120)),
        (json!(1.5), json!(120)),
        (json!(-1), json!(120)),
        (json!(1), json!(31)),
        (json!(1), json!(241)),
        (json!(1), json!("120")),
    ] {
        let invalid = request(&mut socket, 2, "media.thumbs", json!({"media":fixture(),"count":count,"height":height})).await;
        assert_eq!(invalid["error"]["code"], "invalid_params");
        assert!(invalid["error"]["message"].as_str().unwrap().contains("entre"));
    }
    let missing = request(&mut socket, 3, "media.thumbs", json!({"media":data.path().join("missing.mp4"),"count":1,"height":32})).await;
    assert_eq!(missing["error"]["code"], "media_not_found");
    let image = data.path().join("still.png");
    image::RgbImage::from_pixel(16, 9, image::Rgb([255u8, 0, 0])).save(&image).unwrap();
    let unsupported = request(&mut socket, 4, "media.thumbs", json!({"media":image,"count":1,"height":32})).await;
    assert_eq!(unsupported["error"]["code"], "unsupported_media");
    let info = request(&mut socket, 5, "media.probe", json!({"path":fixture()})).await;
    let duration = info["result"]["duration"].as_f64().unwrap();
    let mut saved = Value::Null;
    for (count, height) in [(1, 32), (7, 120), (200, 240)] {
        let result = request(&mut socket, 6, "media.thumbs", json!({"media":fixture(),"count":count,"height":height})).await;
        assert_eq!(result["ok"], true, "{result}");
        let thumbs = result["result"]["thumbs"].as_array().unwrap();
        assert_eq!(thumbs.len(), count);
        for (index, thumb) in thumbs.iter().enumerate() {
            assert!((thumb["time"].as_f64().unwrap() - duration * index as f64 / count as f64).abs() < 0.000001);
            let jpeg = STANDARD.decode(thumb["jpeg"].as_str().unwrap()).unwrap();
            assert_eq!(&jpeg[..3], &[0xff, 0xd8, 0xff]);
            let decoded = image::load_from_memory(&jpeg).unwrap();
            assert_eq!(decoded.height(), height);
            assert_eq!(decoded.width(), (height as f64 * 16.0 / 9.0).round() as u32);
        }
        if count == 7 {
            saved = result["result"].clone();
            assert_ne!(thumbs[0]["jpeg"], thumbs[6]["jpeg"]);
        }
    }
    assert_eq!(std::fs::read_dir(data.path().join("cache")).unwrap().count(), 3);
    request(&mut socket, 99, "shutdown", json!({})).await;
    motor.exited().await;
    let mut restarted = Motor::start_with_data_dir(Some(data.path()));
    let mut socket = restarted.connect().await;
    let disk = request(&mut socket, 7, "media.thumbs", json!({"media":fixture(),"count":7,"height":120})).await;
    assert_eq!(disk["result"], saved);
    let cached = std::fs::read_dir(data.path().join("cache"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|path| {
            let value: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
            value.as_array().unwrap().len() == 7
        })
        .unwrap();
    std::fs::write(cached, b"incompleto").unwrap();
    let rebuilt = request(&mut socket, 8, "media.thumbs", json!({"media":fixture(),"count":7,"height":120})).await;
    assert_eq!(rebuilt["result"], saved);
    let source = data.path().join("replaced.mp4");
    std::fs::copy(fixture(), &source).unwrap();
    let original = request(&mut socket, 9, "media.thumbs", json!({"media":source,"count":1,"height":32})).await;
    assert_eq!(original["ok"], true);
    std::fs::copy(&image, &source).unwrap();
    let changed = request(&mut socket, 10, "media.thumbs", json!({"media":source,"count":1,"height":32})).await;
    assert_eq!(changed["error"]["code"], "unsupported_media");
    request(&mut socket, 99, "shutdown", json!({})).await;
    restarted.exited().await;
}

#[tokio::test]
#[ignore = "Aceite M2.3: MOTOR_THUMBS_MEDIA sintético de 10 min, MOTOR_M23_OUTPUT novo e FFmpeg local."]
async fn thumbs_and_caption_override_acceptance() {
    let media = std::env::var("MOTOR_THUMBS_MEDIA").expect("MOTOR_THUMBS_MEDIA");
    let output = std::path::PathBuf::from(std::env::var("MOTOR_M23_OUTPUT").expect("MOTOR_M23_OUTPUT"));
    std::fs::create_dir_all(&output).unwrap();
    let data = output.join("data");
    let mut motor = Motor::start_with_data_dir(Some(&data));
    let mut socket = motor.connect().await;
    let info = request(&mut socket, 1, "media.probe", json!({"path":media})).await;
    assert!((info["result"]["duration"].as_f64().unwrap() - 600.0).abs() < 0.1);
    assert_eq!(std::fs::read_dir(data.join("cache")).unwrap().count(), 0);
    let mut timings = Vec::new();
    let mut reference = Value::Null;
    for index in 0..6 {
        let start = Instant::now();
        let result = request(&mut socket, 2, "media.thumbs", json!({"media":media,"count":100,"height":120})).await;
        timings.push(start.elapsed().as_secs_f64() * 1000.0);
        assert_eq!(result["ok"], true, "{result}");
        assert_eq!(result["result"]["thumbs"].as_array().unwrap().len(), 100);
        if index == 0 {
            reference = result["result"].clone();
            std::fs::copy(&motor.stderr, output.join("motor-stderr.log")).unwrap();
        } else {
            assert_eq!(result["result"], reference);
        }
    }
    let jpeg = STANDARD.decode(reference["thumbs"][50]["jpeg"].as_str().unwrap()).unwrap();
    std::fs::write(output.join("thumb-50.jpg"), jpeg).unwrap();
    let black = output.join("black.png");
    image::RgbImage::from_pixel(16, 9, image::Rgb([0u8, 0, 0])).save(&black).unwrap();
    let clip = json!({"id":"background","media":black,"track":"V1","start":0,"in":0,"out":2,"volume":1,"fadeIn":0,"fadeOut":0});
    command_frame(&mut socket, 3, "timeline.set", json!({"format":"horizontal","fps":30,"clips":[clip]})).await;
    request(&mut socket, 4, "preview.config", json!({"maxWidth":1920,"maxHeight":1080,"quality":95})).await;
    let mut captions = caption_cases::captions("phrase", 6);
    captions["cues"] = json!([
        {"start":0,"end":1,"words":[{"text":"DARKVOID por frase","start":0,"end":1}],"override":{"position":{"x":0.25,"y":0.3},"size":90,"maxWidth":0.35}},
        {"start":1,"end":2,"words":[{"text":"Estilo original","start":1,"end":2}]}
    ]);
    command_frame(&mut socket, 5, "captions.set", captions.clone()).await;
    let first = command_frame(&mut socket, 6, "transport.seek", json!({"seconds":0.5})).await;
    let second = command_frame(&mut socket, 7, "transport.seek", json!({"seconds":1.5})).await;
    std::fs::write(output.join("override-preview.jpg"), &first).unwrap();
    std::fs::write(output.join("default-preview.jpg"), &second).unwrap();
    let path = output.join("override.mp4");
    let job = request(&mut socket, 8, "export.start", json!({"path":path,"preset":"youtube_1080p","encoder":"nvenc_h264","burnCaptions":true})).await;
    assert_eq!(job["ok"], true, "{job}");
    let done = job_finished(&mut socket, &job["result"]["jobId"]).await;
    assert_eq!(done["event"], "job.done", "{done}");
    let mut errors = Vec::new();
    for (seconds, jpeg) in [(0.5, &first), (1.5, &second)] {
        let expected = image::load_from_memory(jpeg).unwrap().into_rgb8();
        let decoded = Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-ss", &seconds.to_string(), "-i"])
            .arg(&path)
            .args(["-frames:v", "1", "-pix_fmt", "rgb24", "-f", "rawvideo", "pipe:1"])
            .output()
            .unwrap();
        assert!(decoded.status.success());
        assert_eq!(decoded.stdout.len(), expected.as_raw().len());
        let mae = decoded.stdout.iter().zip(expected.as_raw()).map(|(a, b)| u64::from(a.abs_diff(*b))).sum::<u64>() as f64 / decoded.stdout.len() as f64;
        assert!(mae < 2.0, "MAE {mae}");
        let mut caption_error = 0u64;
        let mut caption_pixels = 0usize;
        for (a, b) in decoded.stdout.as_chunks::<3>().0.iter().zip(expected.as_raw().as_chunks::<3>().0) {
            if a.iter().chain(b).any(|v| *v > 32) {
                caption_error += a.iter().zip(b).map(|(a, b)| u64::from(a.abs_diff(*b))).sum::<u64>();
                caption_pixels += 1;
            }
        }
        assert!(caption_pixels > 1000);
        let caption_mae = caption_error as f64 / (caption_pixels * 3) as f64;
        assert!(caption_mae < 6.0, "MAE nos pixels da legenda {caption_mae}");
        errors.push(json!({"seconds":seconds,"maeRgb":mae,"maeCaptionPixels":caption_mae,"captionPixels":caption_pixels}));
    }
    // Um override inválido não substitui a legenda aprovada.
    captions["cues"][0]["override"]["size"] = json!(-1);
    assert_eq!(request(&mut socket, 9, "captions.set", captions).await["error"]["code"], "captions_invalid");
    assert_eq!(command_frame(&mut socket, 10, "transport.seek", json!({"seconds":0.5})).await, first);
    request(&mut socket, 99, "shutdown", json!({})).await;
    motor.exited().await;
    let restart = Instant::now();
    let mut restarted = Motor::start_with_data_dir(Some(&data));
    let mut socket = restarted.connect().await;
    assert_eq!(request(&mut socket, 12, "hello", json!({})).await["result"]["protocol"], 4);
    let restart_startup_ms = restart.elapsed().as_secs_f64() * 1000.0;
    let start = Instant::now();
    let disk = request(&mut socket, 11, "media.thumbs", json!({"media":media,"count":100,"height":120})).await;
    let restarted_ms = start.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(disk["result"], reference);
    request(&mut socket, 99, "shutdown", json!({})).await;
    restarted.exited().await;
    let report = json!({"media":info["result"],"thumbs":100,"height":120,"thumbMs":timings,"restartDiskMs":restarted_ms,"restartStartupMs":restart_startup_ms,"captionComparisons":errors});
    std::fs::write(output.join("measurements.json"), serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    eprintln!("M2.3 {report}");
    assert!(timings[0] < 3000.0, "primeira geração acima de 3 s");
    assert!(timings[1..].iter().all(|ms| *ms < 100.0) && restarted_ms < 100.0, "cache acima de 100 ms");
}
