use crate::{audio_effects, mix::Snapshot, timeline::Timeline};
use filmcraft_engine::{FsServices, Session};
use filmcraft_render::SourceProvider;
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};

fn session() -> Session {
    Session::new(Arc::new(FsServices))
}
fn clip(path: &Path, id: &str, track: &str, start: f64, source_in: f64, out: f64) -> Value {
    json!({"id":id,"media":path,"track":track,"start":start,"in":source_in,"out":out,"volume":1,"fadeIn":0,"fadeOut":0})
}
fn input(clips: Vec<Value>) -> Value {
    json!({"format":"horizontal","fps":1,"clips":clips})
}
fn effect(id: &str, params: Value) -> Value {
    json!({"id":format!("audio.{id}"),"instanceId":"som","enabled":true,"params":params.as_object().unwrap().iter().map(|(k,v)|(k.clone(),json!({"value":v}))).collect::<serde_json::Map<_,_>>()})
}
fn write_audio(path: &Path) {
    let mut bytes = Vec::new();
    for i in 0..(4 * 48000) {
        let t = i as f64 / 48000.0;
        // Harmônicos, sibilância e ruído determinístico, com estéreo não correlacionado.
        for ch in 0..2 {
            let noise = (((i * 7919 + ch * 3719) % 1009) as f64 / 1009.0 - 0.5) * 0.025;
            let sample = 0.12 * (std::f64::consts::TAU * 60.0 * t).sin()
                + 0.08 * (std::f64::consts::TAU * 997.0 * t + ch as f64).sin()
                + 0.08 * (std::f64::consts::TAU * 6000.0 * t + ch as f64 * 0.3).sin()
                + noise;
            bytes.extend((sample as f32).to_le_bytes());
        }
    }
    let len = bytes.len() as u32;
    let mut wav = b"RIFF".to_vec();
    wav.extend((len + 36).to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16u32.to_le_bytes());
    wav.extend(3u16.to_le_bytes());
    wav.extend(2u16.to_le_bytes());
    wav.extend(48000u32.to_le_bytes());
    wav.extend(384000u32.to_le_bytes());
    wav.extend(8u16.to_le_bytes());
    wav.extend(32u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend(len.to_le_bytes());
    wav.extend(bytes);
    std::fs::write(path, wav).unwrap();
}
#[test]
fn catalog_roundtrips_all_audio_defaults_and_translates_every_label() {
    let items = audio_effects::items();
    assert_eq!(items.len(), 19);
    for item in items.iter().filter(|i| i["kind"] == "audioEffect") {
        let params = item["params"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| (p["id"].as_str().unwrap().to_owned(), json!({"value":p["default"]})))
            .collect::<serde_json::Map<_, _>>();
        let e = json!({"id":item["id"],"instanceId":"a","enabled":true,"params":params});
        audio_effects::prepare(&[serde_json::from_value(e).unwrap()], "clipe").unwrap();
    }
}

fn changed(id: &str) -> Value {
    match id {
        "denoise" => json!({"amount":90}),
        "dehummer" => json!({"freq":"1","gain":-70}),
        "speech_enhance" => json!({"mix":100}),
        "parametric_eq" => json!({"mid_gain":-18}),
        "graphic_eq" => json!({"b6":-18}),
        "highpass" => json!({"cutoff":2000}),
        "lowpass" => json!({"cutoff":300}),
        "notch" => json!({"n1_on":true,"n1_freq":997,"n1_gain":-60}),
        "dynamics_rack" => json!({"comp_threshold":-40,"comp_ratio":10}),
        "single_band_compressor" => json!({"threshold":-40,"ratio":10}),
        "multiband_compressor" => json!({"b1_threshold":-40,"b1_ratio":10,"output":-3}),
        "hard_limiter" => json!({"max":-18}),
        "deesser" => json!({"threshold":-40,"reduction":24}),
        "amplify" => json!({"gain":-12}),
        "bass" | "treble" => json!({"boost":-18}),
        "stereo_width" => json!({"width":0}),
        "balance_a" => json!({"balance":80}),
        _ => unreachable!(),
    }
}
fn preview(session: &Session) -> Vec<Vec<f32>> {
    let pool = Arc::new(filmcraft_engine::MediaPool::default());
    let sources = pool.provider(session.project.clone(), session.services.clone());
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
fn exported(session: &Session, path: &Path) -> Vec<Vec<f32>> {
    let settings = filmcraft_export::ExportSettings {
        path: path.to_string_lossy().into_owned(),
        format: filmcraft_export::Format::Wav,
        audio: filmcraft_export::AudioSettings { bits: 32, float: true, sample_rate: Some(48000), ..Default::default() },
        ..Default::default()
    };
    let sources = session.media.provider(session.project.clone(), session.services.clone());
    filmcraft_export::export(&session.project, session.state.active_sequence.unwrap(), &settings, &sources, &filmcraft_export::Progress::default()).unwrap();
    let source = session.media.probe_file(path.to_str().unwrap(), &*session.services).unwrap();
    source.audio(0, session.active_sequence().unwrap().duration().to_units_floor(48000) as usize, 48000).unwrap().channels
}
fn case(id: &str) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sinal.wav");
    write_audio(&path);
    let mut s = session();
    let mut timeline = Timeline::default();
    let mut c = clip(&path, "som", "A1", 0.0, 0.0, 4.0);
    timeline.set(&mut s, input(vec![c.clone()])).unwrap();
    let baseline = preview(&s);
    c["audioEffects"] = json!([effect(id, changed(id))]);
    timeline.set(&mut s, input(vec![c])).unwrap();
    let expected = preview(&s);
    let difference = expected.iter().flatten().zip(baseline.iter().flatten()).map(|(a, b)| f64::from(a - b).powi(2)).sum::<f64>() / (2 * 4 * 48000) as f64;
    assert!(difference.sqrt() > 0.00001, "{id} não muda o áudio");
    let actual = exported(&s, &dir.path().join("exportado.wav"));
    let error = expected.iter().flatten().zip(actual.iter().flatten()).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
    assert_eq!(actual[0].len(), expected[0].len());
    assert!(error <= 0.000001, "{id}: erro máximo de amostra {error}");
    eprintln!("M4.2 {id}: alteração RMS {:.6}; erro máximo prévia/WAV {error}", difference.sqrt());
}
macro_rules! cases { ($($id:ident),+ $(,)?) => { $(#[test] fn $id() { case(stringify!($id)); })+ }; }
cases!(
    denoise,
    dehummer,
    speech_enhance,
    parametric_eq,
    graphic_eq,
    highpass,
    lowpass,
    notch,
    dynamics_rack,
    single_band_compressor,
    multiband_compressor,
    hard_limiter,
    deesser,
    amplify,
    bass,
    treble,
    stereo_width,
    balance_a
);

#[test]
fn ducking_tracks_reduce_by_requested_db_and_recompute_on_edits() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sinal.wav");
    write_audio(&path);
    let mut s = session();
    let mut timeline = Timeline::default();
    let voice = clip(&path, "voz", "A1", 1.0, 0.0, 2.0);
    let music = clip(&path, "música", "A2", 0.0, 0.0, 4.0);
    let mut params = input(vec![voice.clone(), music.clone()]);
    timeline.set(&mut s, params.clone()).unwrap();
    let baseline = preview(&s);
    params["tracks"] = json!({"A2":{"duck":{"by":["A1"],"reduce":18,"sensitivity":50,"fade":0.2}}});
    timeline.set(&mut s, params.clone()).unwrap();
    let ducked = preview(&s);
    let track = &s.active_sequence().unwrap().audio_tracks[3];
    let lane = &track.mixer.lanes["volume"];
    assert!((lane.value_at(filmcraft_time::Tick::from_seconds_f64(2.0)).as_f64().unwrap() + 18.0).abs() < 0.001);
    assert_eq!(lane.value_at(filmcraft_time::Tick::from_seconds_f64(0.2)).as_f64().unwrap(), 0.0);
    assert_eq!(lane.value_at(filmcraft_time::Tick::from_seconds_f64(3.8)).as_f64().unwrap(), 0.0);
    // Isola a música mantendo a automação calculada para medir sua redução.
    let seq = s.state.active_sequence.unwrap();
    Arc::make_mut(&mut s.project).sequence_mut(seq).unwrap().audio_tracks[2].muted = true;
    let just_music = preview(&s);
    let mut no_duck = session();
    Timeline::default().set(&mut no_duck, input(vec![music])).unwrap();
    let plain_music = preview(&no_duck);
    let rms = |v: &[f32]| (v.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>() / v.len() as f64).sqrt();
    let delta = 20.0 * (rms(&just_music[0][72000..120000]) / rms(&plain_music[0][72000..120000])).log10();
    assert!((delta + 18.0).abs() < 0.01, "redução {delta} dB");
    Arc::make_mut(&mut s.project).sequence_mut(seq).unwrap().audio_tracks[2].muted = false;
    assert!(ducked != baseline);
    let actual = exported(&s, &dir.path().join("duck.wav"));
    assert_eq!(ducked, actual);
    params["clips"][0]["muted"] = json!(true);
    timeline.set(&mut s, params.clone()).unwrap();
    assert!(s.active_sequence().unwrap().audio_tracks[3].mixer.lanes["volume"].keyframes.iter().all(|k| k.value.as_f64() == Some(0.0)));
    params.as_object_mut().unwrap().remove("tracks");
    timeline.set(&mut s, params).unwrap();
    assert!(s.active_sequence().unwrap().audio_tracks[3].mixer.lanes.is_empty());
    eprintln!("M4.2 ducking: {delta:.6} dB; prévia/WAV idênticos");
}

#[test]
fn constant_power_crossfade_uses_handles_and_has_no_click() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sinal.wav");
    write_audio(&path);
    let mut s = session();
    let mut timeline = Timeline::default();
    let a = clip(&path, "a", "A1", 0.0, 0.5, 2.5);
    let mut b = clip(&path, "b", "A1", 2.0, 0.5, 2.5);
    b["crossfadeIn"] = json!(1.0);
    let params = input(vec![a, b]);
    timeline.set(&mut s, params).unwrap();
    let snapshot = Snapshot::new(&s).unwrap();
    let read = snapshot.reader(48000);
    let provider = s.media.provider(s.project.clone(), s.services.clone());
    let item = &s.active_sequence().unwrap().audio_tracks[2].items[0];
    let src = provider.source(item.item).unwrap();
    for i in [72000, 84000, 96000, 108000, 119000] {
        let p = (i - 72000) as f32 / 48000.0;
        let a = src.audio(i + 24000, 1, 48000).unwrap();
        let b = src.audio(i - 72000, 1, 48000).unwrap();
        let signal = read(i, 1);
        for ch in 0..2 {
            let expected = a.channels[ch][0] * (p * std::f32::consts::FRAC_PI_2).cos() + b.channels[ch][0] * (p * std::f32::consts::FRAC_PI_2).sin();
            assert!((signal[ch][0] - expected).abs() < 0.000001, "amostra {i}, canal {ch}: {} versus {expected}", signal[ch][0]);
        }
    }
    let expected = preview(&s);
    let actual = exported(&s, &dir.path().join("cruzamento.wav"));
    assert_eq!(expected, actual);
    // A fonte contém componentes de 6 kHz: testa a diferença contra o próprio sinal
    // analítico do cruzamento, em vez de confundir a derivada do áudio com um estalo.
    let whole = read(71999, 48002);
    let split: Vec<_> = (0..48002).step_by(137).flat_map(|i| read(71999 + i as i64, (48002 - i).min(137))[0].clone()).collect();
    assert_eq!(whole[0], split);
}

#[test]
fn crossfade_has_continuous_edges_on_constant_signal() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("constante.wav");
    write_audio(&path);
    let mut bytes = std::fs::read(&path).unwrap();
    for sample in bytes[44..].chunks_exact_mut(4) {
        sample.copy_from_slice(&0.1f32.to_le_bytes());
    }
    std::fs::write(&path, bytes).unwrap();
    let mut s = session();
    let mut timeline = Timeline::default();
    let a = clip(&path, "a", "A1", 0.0, 0.5, 2.5);
    let mut b = clip(&path, "b", "A1", 2.0, 0.5, 2.5);
    b["crossfadeIn"] = json!(1.0);
    timeline.set(&mut s, input(vec![a, b])).unwrap();
    let signal = preview(&s);
    let largest = signal[0][71990..120010].windows(2).map(|s| (s[1] - s[0]).abs()).fold(0.0f32, f32::max);
    assert!(largest < 0.00001, "salto no cruzamento: {largest}");
    let mid = signal[0][96000];
    assert!((mid - 0.1 * 2.0f32.sqrt()).abs() < 0.000001);
    eprintln!("M4.2 cruzamento: maior salto DC {largest:.9}; meio {mid:.9}");
}

#[test]
fn audio_keyframes_disabled_order_video_and_atomic_validation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sinal.wav");
    write_audio(&path);
    let mut s = session();
    let mut timeline = Timeline::default();
    let mut c = clip(&path, "a", "A1", 0.0, 1.0, 3.0);
    let mut e = effect("amplify", json!({"gain":0}));
    e["params"]["gain"]["keyframes"] = json!([
        {"time":1,"value":-12,"interpolation":"hold"},
        {"time":2,"value":0,"interpolation":"hold"}]);
    c["audioEffects"] = json!([e.clone()]);
    timeline.set(&mut s, input(vec![c.clone()])).unwrap();
    let animated = preview(&s);
    assert_eq!(animated, exported(&s, &dir.path().join("animado.wav")));
    e["enabled"] = json!(false);
    c["audioEffects"] = json!([e.clone()]);
    timeline.set(&mut s, input(vec![c.clone()])).unwrap();
    let plain = preview(&s);
    let rms = |v: &[f32]| (v.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>() / v.len() as f64).sqrt();
    assert!((20.0 * (rms(&animated[0][24000..36000]) / rms(&plain[0][24000..36000])).log10() + 12.0).abs() < 0.01);
    assert!(rms(&animated[0][72000..84000]) > rms(&animated[0][24000..36000]) * 3.0);
    let before = s.project.clone();
    for invalid in [
        json!([effect("unknown", json!({}))]),
        json!([effect("amplify", json!({"unknown":0}))]),
        json!([effect("amplify", json!({"gain":25}))]),
        json!([effect("hard_limiter", json!({"lookahead":5}))]),
        json!([effect("hard_limiter", json!({"boost":-1}))]),
        json!([e.clone(), e.clone()]),
        json!([{"id":"audio.amplify","instanceId":"a","enabled":true,"params":{"gain":{"value":0,"keyframes":[{"time":1,"value":0,"interpolation":"linear"},{"time":1,"value":1,"interpolation":"linear"}]}}}]),
    ] {
        c["audioEffects"] = invalid;
        let err = timeline.set(&mut s, input(vec![c.clone()])).unwrap_err();
        assert_eq!(err.code, "timeline_invalid");
        assert!(Arc::ptr_eq(&s.project, &before));
    }
    c["audioEffects"] = json!([]);
    for crossfade in [json!(0), json!(-1), json!(4), json!("1")] {
        c["crossfadeIn"] = crossfade;
        assert!(timeline.set(&mut s, input(vec![c.clone()])).is_err());
        assert!(Arc::ptr_eq(&s.project, &before));
    }
    c.as_object_mut().unwrap().remove("crossfadeIn");
    for tracks in
        [json!({"V1":{}}), json!({"A2":{"duck":{"by":["A2"]}}}), json!({"A2":{"duck":{"by":["A1","A1"]}}}), json!({"A2":{"duck":{"by":["A1"],"reduce":41}}})]
    {
        let mut p = input(vec![c.clone()]);
        p["tracks"] = tracks;
        assert!(timeline.set(&mut s, p).is_err());
        assert!(Arc::ptr_eq(&s.project, &before));
    }
    // Os efeitos também acompanham o áudio vinculado de um clipe de vídeo.
    let video = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/padrao.mp4");
    let mut v = clip(&video, "vídeo", "V1", 0.0, 0.0, 2.0);
    timeline.set(&mut s, input(vec![v.clone()])).unwrap();
    let baseline = preview(&s);
    v["audioEffects"] = json!([effect("amplify", json!({"gain":-12}))]);
    timeline.set(&mut s, input(vec![v])).unwrap();
    let processed = preview(&s);
    assert!((rms(&processed[0][24000..48000]) / rms(&baseline[0][24000..48000]) - 10.0f64.powf(-12.0 / 20.0)).abs() < 0.001);
    // Ordem é preservada mesmo quando duas instâncias usam o mesmo tipo.
    c["audioEffects"] =
        json!([effect("amplify",json!({"gain":-6})),{"id":"audio.hard_limiter","instanceId":"lim","enabled":true,"params":{"max":{"value":-18}}}]);
    timeline.set(&mut s, input(vec![c.clone()])).unwrap();
    let first = preview(&s);
    c["audioEffects"].as_array_mut().unwrap().reverse();
    timeline.set(&mut s, input(vec![c])).unwrap();
    assert_ne!(first, preview(&s));
}
