//! Aceite M4.5 com mídia sintética e o caminho real de mistura/exportação.
use crate::{mix::Snapshot, timeline::Timeline};
use filmcraft_engine::{FsServices, Session};
use filmcraft_export::{ExportSettings, Format, Progress, SampleFormat};
use filmcraft_time::Tick;
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}
fn clip(path: &Path, track: &str) -> Value {
    json!({"id":"separado","media":path,"track":track,"start":0,"in":0,"out":3,
        "volume":1,"fadeIn":0,"fadeOut":0,"audioOnly":true})
}
fn input(clips: Vec<Value>) -> Value {
    json!({"format":"horizontal","clips":clips})
}
fn session() -> Session {
    Session::new(Arc::new(FsServices))
}
fn preview(session: &Session) -> Vec<Vec<f32>> {
    let sources = Arc::new(filmcraft_engine::MediaPool::default()).provider(session.project.clone(), session.services.clone());
    let queue = crossbeam_queue::ArrayQueue::new(1024);
    let mut cursor = 0;
    let mut channels = vec![Vec::new(), Vec::new()];
    let total = session.active_sequence().unwrap().duration().to_units_floor(48000) as usize;
    for pos in (0..total).step_by(1024) {
        crate::audio::fill(&session.project, session.state.active_sequence.unwrap(), &sources, &queue, &mut cursor, (total - pos).min(1024));
        while let Some(frame) = queue.pop() {
            for ch in 0..2 {
                channels[ch].push(frame[ch]);
            }
        }
    }
    channels
}

#[test]
fn audio_only_validation_is_atomic_and_v4_stays_compatible() {
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join("imagem.png");
    image::RgbImage::new(8, 8).save(&image).unwrap();
    let audio = dir.path().join("audio.wav");
    let settings = ExportSettings { path: audio.to_string_lossy().into_owned(), format: Format::Wav, ..Default::default() };
    // Gera áudio puro a partir da mesma mídia pública sintética, sem ferramentas externas.
    let mut s = session();
    let mut timeline = Timeline::default();
    timeline.set(&mut s, input(vec![clip(&fixture("padrao.mp4"), "A1")])).unwrap();
    let sources = s.media.provider(s.project.clone(), s.services.clone());
    filmcraft_export::export(&s.project, s.state.active_sequence.unwrap(), &settings, &sources, &Progress::default()).unwrap();
    let before = s.project.clone();
    for (path, track, reason) in [
        (fixture("sem-audio.mp4"), "A1", "arquivo sem áudio"),
        (image, "A2", "imagem não tem áudio"),
        (audio.clone(), "A1", "arquivo sem áudio"),
        (fixture("padrao.mp4"), "V1", "trilha errada para o tipo de mídia"),
        (fixture("padrao.mp4"), "V2", "trilha errada para o tipo de mídia"),
    ] {
        let e = timeline.set(&mut s, input(vec![clip(&path, track)])).unwrap_err();
        assert_eq!(e.code, "timeline_invalid");
        assert!(e.message.contains("separado") && e.message.contains(reason), "{}", e.message);
        assert!(Arc::ptr_eq(&before, &s.project));
    }
    for flag in [None, Some(false)] {
        let mut c = clip(&fixture("padrao.mp4"), "V1");
        c.as_object_mut().unwrap().remove("audioOnly");
        if let Some(flag) = flag {
            c["audioOnly"] = json!(flag);
        }
        timeline.set(&mut s, input(vec![c.clone()])).unwrap();
        assert_eq!(s.active_sequence().unwrap().video_tracks[0].items.len(), 1);
        c["track"] = json!("A1");
        assert_eq!(timeline.set(&mut s, input(vec![c])).unwrap_err().code, "timeline_invalid");
    }
    let mut c = clip(&audio, "A1");
    c["audioOnly"] = json!(false);
    timeline.set(&mut s, input(vec![c])).unwrap();
}

#[test]
fn audio_only_pcm_export_parity_muted_video_and_speed_keep_pitch() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = session();
    let mut timeline = Timeline::default();
    for speed in [0.5, 1.0, 2.0] {
        let mut c = clip(&fixture("padrao.mp4"), "A2");
        c["in"] = json!(0.25);
        c["out"] = json!(2.75);
        c["speed"] = json!(speed);
        timeline.set(&mut s, input(vec![c.clone()])).unwrap();
        let seq = s.active_sequence().unwrap();
        assert!(seq.video_tracks.iter().all(|t| t.items.is_empty()));
        assert_eq!(seq.duration(), Tick::from_seconds_f64(2.5 / speed));
        let frames = seq.duration().to_units_floor(48000) as usize;
        let expected = preview(&s);
        assert_eq!(expected.len(), 2);
        let crossings = expected[0][4800..28800].windows(2).filter(|p| p[0] <= 0.0 && p[1] > 0.0).count();
        assert!((crossings as f64 * 2.0 - 440.0).abs() < 12.0, "speed={speed}");
        let mut video = c.clone();
        video["id"] = json!("quadro");
        video["audioOnly"] = json!(false);
        video["track"] = json!("V1");
        video["muted"] = json!(true);
        timeline.set(&mut s, input(vec![c.clone(), video])).unwrap();
        assert!(s.active_sequence().unwrap().video_tracks[0].items[0].enabled);
        assert!(expected == preview(&s), "vídeo mudo não deve duplicar o áudio");
        for format in [SampleFormat::Float32, SampleFormat::Int24] {
            let path = dir.path().join(format!("pcm-{speed}-{format:?}.wav"));
            let mut settings = ExportSettings { path: path.to_string_lossy().into_owned(), format: Format::Wav, ..Default::default() };
            settings.audio.set_sample_format(format);
            let sources = Arc::new(filmcraft_engine::MediaPool::default()).full_res_provider(s.project.clone(), s.services.clone());
            filmcraft_export::export(&s.project, s.state.active_sequence.unwrap(), &settings, &sources, &Progress::default()).unwrap();
            let decoded = s.media.probe_file(path.to_str().unwrap(), &*s.services).unwrap();
            assert_eq!(decoded.info().audio().unwrap().sample_rate, 48000);
            assert_eq!(decoded.info().audio().unwrap().channels, 2);
            assert_eq!(decoded.info().duration.to_units_floor(48000), frames as i64);
            let actual = decoded.audio(0, frames, 48000).unwrap().channels;
            if format == SampleFormat::Float32 {
                assert!(expected == actual, "PCM deve ser idêntico antes da quantização: speed={speed}");
            } else {
                let max_error = expected.iter().flatten().zip(actual.iter().flatten()).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
                assert!(max_error <= 1.2e-7, "quantização PCM24: {max_error}");
            }
        }
        let mut next = c.clone();
        next["id"] = json!("seguinte");
        next["start"] = json!(2.5 / speed);
        timeline.set(&mut s, input(vec![c.clone(), next.clone()])).unwrap();
        next["start"] = json!(2.5 / speed - 0.1);
        assert_eq!(timeline.set(&mut s, input(vec![c.clone(), next])).unwrap_err().code, "timeline_invalid");
        c["out"] = json!(4);
        assert_eq!(timeline.set(&mut s, input(vec![c])).unwrap_err().code, "timeline_invalid");
    }
}

#[test]
#[ignore = "Aceite M4.5: requer FFmpeg local apenas para gerar os contêineres sintéticos."]
fn audio_only_video_containers_decode_without_video_rendering() {
    let dir = tempfile::tempdir().unwrap();
    for (extension, video, audio, extra) in [
        ("mp4", "libx264", "aac", vec![]),
        ("mov", "libx264", "aac", vec![]),
        ("mkv", "libx264", "aac", vec![]),
        ("webm", "libvpx-vp9", "libopus", vec![]),
        ("avi", "mjpeg", "pcm_s16le", vec![]),
        ("ts", "libx264", "aac", vec![]),
        ("mpg", "mpeg2video", "mp2", vec!["-f", "mpeg"]),
        ("mxf", "mpeg2video", "pcm_s16le", vec!["-ar", "48000"]),
    ] {
        let path = dir.path().join(format!("sintetico.{extension}"));
        let size = if extension == "mxf" { "720x576" } else { "64x48" };
        let result = std::process::Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-n",
                "-f",
                "lavfi",
                "-i",
                &format!("color=c=blue:s={size}:r=25"),
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=48000",
                "-t",
                "3",
                "-c:v",
                video,
                "-c:a",
                audio,
            ])
            .args(extra)
            .arg(&path)
            .output()
            .unwrap();
        assert!(result.status.success(), "{extension}: {}", String::from_utf8_lossy(&result.stderr));
        let mut s = session();
        let mut timeline = Timeline::default();
        let mut c = clip(&path, "A1");
        c["in"] = json!(0.5);
        c["out"] = json!(2.5);
        timeline.set(&mut s, input(vec![c])).unwrap_or_else(|e| panic!("{extension}: {}", e.message));
        assert!(s.active_sequence().unwrap().video_tracks.iter().all(|t| t.items.is_empty()));
        // Lê diretamente o decoder para não confundir falha de decode com silêncio do mixer.
        let source = s.media.probe_file(path.to_str().unwrap(), &*s.services).unwrap();
        let decoded = source.audio(24000, 48000, 48000).unwrap_or_else(|e| panic!("{extension}: {e}"));
        assert!(decoded.channels.iter().flatten().any(|v| v.abs() > 0.01), "{extension}: silêncio inesperado");
        let signal = preview(&s);
        let crossings = signal[0][4800..52800].windows(2).filter(|p| p[0] <= 0.0 && p[1] > 0.0).count();
        assert!((crossings as f64 - 440.0).abs() < 12.0, "{extension}: {crossings} Hz");
        let output = dir.path().join(format!("saida-{extension}.wav"));
        let mut settings = ExportSettings { path: output.to_string_lossy().into_owned(), format: Format::Wav, ..Default::default() };
        settings.audio.set_sample_format(SampleFormat::Float32);
        let sources = Arc::new(filmcraft_engine::MediaPool::default()).full_res_provider(s.project.clone(), s.services.clone());
        filmcraft_export::export(&s.project, s.state.active_sequence.unwrap(), &settings, &sources, &Progress::default()).unwrap();
        let exported = s.media.probe_file(output.to_str().unwrap(), &*s.services).unwrap();
        assert!(signal == exported.audio(0, 96000, 48000).unwrap().channels, "{extension}: PCM divergente");
        eprintln!("M4.5 {extension}/{audio}: decode de áudio, 48 kHz estéreo e paridade PCM aprovados");
    }
}

#[test]
#[ignore = "Bloqueio M4.5: decoder AAC compartilhado não preserva PCM entre tamanhos de blocos; manter a reprodução para investigação."]
fn audio_only_crossfade_and_ducking_share_export_mix() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = session();
    let mut timeline = Timeline::default();
    let path = fixture("padrao.mp4");
    let whole = s.media.probe_file(path.to_str().unwrap(), &*s.services).unwrap();
    let blocks = s.media.probe_file(path.to_str().unwrap(), &*s.services).unwrap();
    let reference = whole.audio(12000, 72000, 48000).unwrap();
    blocks.audio(12000, 1024, 48000).unwrap();
    blocks.audio(0, 1024, 48000).unwrap();
    let second = blocks.audio(13024, 1024, 48000).unwrap();
    let decoder_error = reference.channels[0][1024..2048].iter().zip(&second.channels[0]).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
    eprintln!("M4.5 decoder MP4/AAC, acesso intercalado sem mixer: erro máximo {decoder_error}");
    let mut a = clip(&fixture("padrao.mp4"), "A1");
    a["in"] = json!(0.25);
    a["out"] = json!(1.75);
    let mut b = a.clone();
    b["id"] = json!("seguinte");
    b["start"] = json!(1.5);
    b["crossfadeIn"] = json!(0.4);
    let mut bed = clip(&fixture("padrao.mp4"), "A2");
    bed["id"] = json!("fundo");
    bed["volume"] = json!(0.25);
    let mut params = input(vec![a.clone(), b.clone(), bed]);
    timeline.set(&mut s, params.clone()).unwrap();
    let plain = preview(&s);
    params["tracks"] = json!({"A2":{"duck":{"by":["A1"],"reduce":18,"sensitivity":100,"fade":0.1}}});
    timeline.set(&mut s, params).unwrap();
    let ducked = preview(&s);
    assert_ne!(plain, ducked, "ducking deve alterar a mistura do áudio separado");
    let path = dir.path().join("cruzamento-ducking.wav");
    let mut settings = ExportSettings { path: path.to_string_lossy().into_owned(), format: Format::Wav, ..Default::default() };
    settings.audio.set_sample_format(SampleFormat::Float32);
    let sources = Arc::new(filmcraft_engine::MediaPool::default()).full_res_provider(s.project.clone(), s.services.clone());
    filmcraft_export::export(&s.project, s.state.active_sequence.unwrap(), &settings, &sources, &Progress::default()).unwrap();
    let exported = s.media.probe_file(path.to_str().unwrap(), &*s.services).unwrap();
    let actual = exported.audio(0, 144000, 48000).unwrap().channels;
    let differences: Vec<_> =
        ducked.iter().flatten().zip(actual.iter().flatten()).enumerate().filter(|(_, (a, b))| a != b).map(|(i, (a, b))| (i, *a, *b)).collect();
    let max_error = differences.iter().map(|(_, a, b)| (a - b).abs()).fold(0.0f32, f32::max);
    assert!(
        differences.is_empty(),
        "{} amostras diferentes; erro máximo {max_error}; primeiras: {:?}",
        differences.len(),
        &differences[..differences.len().min(5)]
    );
    timeline.set(&mut s, input(vec![a.clone(), b.clone()])).unwrap();
    let crossfade = Snapshot::new(&s).unwrap().reader(48000)(60000, 24000);
    b.as_object_mut().unwrap().remove("crossfadeIn");
    timeline.set(&mut s, input(vec![a, b])).unwrap();
    let cut = Snapshot::new(&s).unwrap().reader(48000)(60000, 24000);
    assert_ne!(crossfade, cut, "crossfadeIn deve alterar o encontro dos clipes audioOnly");
}
