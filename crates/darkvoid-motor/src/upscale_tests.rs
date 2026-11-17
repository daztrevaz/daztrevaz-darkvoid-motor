//! Aceite clássico: filtro real, contrato, paridade e referência física 1080p → 2160p.
use filmcraft_engine::{FsServices, Session};
use filmcraft_time::{FrameRate, Tick};
use serde_json::json;
use std::{
    path::Path,
    sync::{Arc, atomic::AtomicU64},
    time::{Duration, Instant},
};

fn session(path: &Path) -> Session {
    let mut s = Session::new(Arc::new(FsServices));
    let imported = s.execute("file.import", json!({"paths":[path]})).unwrap();
    s.execute("file.newSequenceFromClip", json!({"items":imported["items"]})).unwrap();
    s
}
fn fixture(path: &Path, w: u32, h: u32) {
    image::RgbImage::from_fn(w, h, |x, y| {
        let value = if x >= w / 3 && x < 2 * w / 3 {
            (40 + 170 * (x - w / 3) / (w / 3)) as u8
        } else {
            // Supersample the analytic shapes so the reference contains no binary raster aliasing.
            let mut bright = 0;
            for j in 0..4 {
                for i in 0..4 {
                    let px = x as f64 + (i as f64 + 0.5) / 4.0;
                    let py = y as f64 + (j as f64 + 0.5) / 4.0;
                    let on = if x < w / 3 {
                        px > py * 0.63
                    } else {
                        let dx = px - 5.0 * w as f64 / 6.0;
                        let dy = py - h as f64 / 2.0;
                        (dx.hypot(dy) / 10.0).floor() as u32 % 2 == 0
                    };
                    bright += u32::from(on);
                }
            }
            (40 + 170 * bright / 16) as u8
        };
        image::Rgb([value, value, value])
    })
    .save(path)
    .unwrap();
}
#[test]
fn classic_catalog_validation_and_pixels_are_effective() {
    let _guard = crate::effects_tests::GPU_TEST.lock().unwrap_or_else(|e| e.into_inner());
    let cat = crate::catalog::get();
    let item = cat["items"].as_array().unwrap().iter().find(|i| i["id"] == "scale.classic").unwrap();
    assert_eq!(item["params"][0]["min"], 0);
    assert_eq!(item["params"][0]["max"], 100);
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("fonte.png");
    fixture(&source, 96, 54);
    let s = session(&source);
    let sources = s.media.provider(s.project.clone(), s.services.clone());
    let mut gpu = filmcraft_gpu::ExportRenderer::new();
    let mut frames = Vec::new();
    for (name, sharpness) in [("bilinear", None), ("classic0", Some(0.0)), ("classic100", Some(100.0))] {
        let target = dir.path().join(format!("{name}.png"));
        let settings = filmcraft_export::ExportSettings {
            path: target.to_string_lossy().into_owned(),
            format: filmcraft_export::Format::PngSequence,
            frame_size: Some((192, 108)),
            frame_rate: Some(FrameRate::new(1, 1)),
            range: Some(filmcraft_time::TimeRange::new(Tick::ZERO, Tick::from_seconds_f64(1.0))),
            include_audio: false,
            classic_scale: sharpness,
            max_threads: 2,
            ..Default::default()
        };
        filmcraft_export::export(&s.project, s.state.active_sequence.unwrap(), &settings, &sources, &Default::default()).unwrap();
        frames.push(image::open(filmcraft_export::image_sequence_path(&target.to_string_lossy(), 0, 1)).unwrap().to_rgb8().into_raw());
        if let (Some(sharpness), Some(gpu)) = (sharpness, gpu.as_mut()) {
            let opts = filmcraft_render::RenderOptions { scale: 2.0, classic_scale: Some(sharpness), ..Default::default() };
            let a = filmcraft_render::render_sequence(&s.project, s.state.active_sequence.unwrap(), Tick::ZERO, opts, &sources).over_black_rgba8();
            let b = gpu.render(&s.project, s.state.active_sequence.unwrap(), Tick::ZERO, opts, &sources).unwrap().over_black_rgba8();
            assert!(a.iter().zip(b).all(|(a, b)| a.abs_diff(b) <= 2));
        }
    }
    assert_ne!(frames[0], frames[1]);
    assert_ne!(frames[1], frames[2]);
    for frame in &frames[1..] {
        assert!(frame.iter().all(|p| *p >= 40 && *p <= 210));
    }
    let (events, _) = tokio::sync::broadcast::channel(8);
    let mut exports = crate::exports::Exports::new(dir.path().into(), events, Arc::new(AtomicU64::new(1)));
    for scale in [json!({"method":"other","sharpness":0}), json!({"method":"classic","sharpness":-1}), json!({"method":"classic","sharpness":101})] {
        let params =
            serde_json::from_value(json!({"path":dir.path().join("invalid.mp4"),"preset":"youtube_2160p","encoder":"software","scale":scale})).unwrap();
        assert_eq!(exports.start(&s, params).unwrap_err().code, "invalid_params");
        assert!(!dir.path().join("invalid.mp4").exists());
    }
}

#[test]
#[ignore = "Aceite físico M3.4: GPU real e FFmpeg local apenas para criar a referência sintética"]
fn reference_1080p_to_2160p_export_and_timing() {
    let _guard = crate::effects_tests::GPU_TEST.lock().unwrap_or_else(|e| e.into_inner());
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/m3.4-reference");
    std::fs::create_dir_all(&root).unwrap();
    let run = root.join(format!("run-{}", std::process::id()));
    std::fs::create_dir(&run).unwrap();
    let reference = run.join("referencia-1080p.png");
    fixture(&reference, 1920, 1080);
    let video = run.join("referencia-1080p.mp4");
    let ffmpeg = std::env::var("MOTOR_M34_FFMPEG").expect("MOTOR_M34_FFMPEG");
    let status = std::process::Command::new(ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-loop", "1", "-i"])
        .arg(&reference)
        .args(["-t", "0.4", "-r", "5", "-c:v", "libx264", "-crf", "12", "-pix_fmt", "yuv420p"])
        .arg(&video)
        .status()
        .unwrap();
    assert!(status.success());
    let s = session(&video);
    let sources = s.media.provider(s.project.clone(), s.services.clone());
    let source = s.media.probe_file(video.to_str().unwrap(), &*s.services).unwrap();
    assert_eq!((source.info().video.as_ref().unwrap().width, source.info().video.as_ref().unwrap().height), (1920, 1080));
    let mut gpu = filmcraft_gpu::ExportRenderer::new().expect("GPU real, sem fallback de aceite");
    let probe = crate::preview::measurement::Probe::new(true);
    eprintln!("M3.4 GPU: {}", probe.info());
    let opts = filmcraft_render::RenderOptions { scale: 2.0, classic_scale: Some(35.0), ..Default::default() };
    let cpu = filmcraft_render::render_sequence(&s.project, s.state.active_sequence.unwrap(), Tick::ZERO, opts, &sources).over_black_rgba8();
    let actual = gpu.render(&s.project, s.state.active_sequence.unwrap(), Tick::ZERO, opts, &sources).unwrap().over_black_rgba8();
    let mut diffs: Vec<_> = cpu.chunks_exact(4).zip(actual.chunks_exact(4)).map(|(a, b)| (0..3).map(|k| a[k].abs_diff(b[k]) as u32).max().unwrap()).collect();
    diffs.sort_unstable();
    let mean = diffs.iter().sum::<u32>() as f64 / diffs.len() as f64;
    let p99 = diffs[diffs.len() * 99 / 100];
    eprintln!("M3.4 GPU/CPU: média {mean:.4}, p99 {p99}");
    assert!(mean < 1.5 && p99 <= 6);
    image::save_buffer(run.join("classico-cpu-2160p.png"), &cpu, 3840, 2160, image::ColorType::Rgba8).unwrap();
    image::save_buffer(run.join("classico-gpu-2160p.png"), &actual, 3840, 2160, image::ColorType::Rgba8).unwrap();
    let target = run.join("classico-2160p.mp4");
    let (events, mut received) = tokio::sync::broadcast::channel(64);
    let mut exports = crate::exports::Exports::new(run.clone(), events, Arc::new(AtomicU64::new(1)));
    let encoder = if exports.capabilities.h264 { "nvenc_h264" } else { "software" };
    let began = Instant::now();
    let params = serde_json::from_value(json!({"path":target,"preset":"youtube_2160p","encoder":encoder,"scale":{"method":"classic","sharpness":35}})).unwrap();
    let before = filmcraft_export::gpu_render_stats();
    exports.start(&s, params).unwrap();
    loop {
        exports.poll();
        if let Ok(event) = received.try_recv() {
            assert_ne!(event["event"], "job.error", "{event}");
            if event["event"] == "job.done" {
                break;
            }
        }
        assert!(began.elapsed() < Duration::from_secs(600));
        std::thread::sleep(Duration::from_millis(20));
    }
    let elapsed = began.elapsed().as_secs_f64();
    let stats = filmcraft_export::gpu_render_stats();
    assert!(stats.frames - before.frames >= 2);
    assert_eq!(stats.fallbacks - before.fallbacks, 0);
    let result = s.media.probe_file(target.to_str().unwrap(), &*s.services).unwrap();
    let v = result.info().video.as_ref().unwrap();
    assert_eq!((v.width, v.height), (3840, 2160));
    let frame = result.video_frame(filmcraft_media::FrameRequest::full(Tick::ZERO)).unwrap().to_rgba8();
    let encoded_mae = frame.chunks_exact(4).zip(cpu.chunks_exact(4)).flat_map(|(a, b)| (0..3).map(move |k| u64::from(a[k].abs_diff(b[k])))).sum::<u64>() as f64
        / (3840.0 * 2160.0 * 3.0);
    assert!(encoded_mae < 3.0, "erro RGB do MP4: {encoded_mae}");
    image::save_buffer(run.join("exportado-2160p.png"), &frame, 3840, 2160, image::ColorType::Rgba8).unwrap();
    let report = json!({"seconds":elapsed,"encoder":encoder,"source":[1920,1080],"output":[3840,2160],"duration":0.4,"frames":2,"sharpness":35,"gpuMean":mean,"gpuP99":p99,"encodedMae":encoded_mae});
    std::fs::write(run.join("resultado.json"), serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    eprintln!("M3.4: {report}; artefatos: {}", run.display());
}
