//! Medição física M3.0, sem comandos novos no protocolo e sem alterações no produto.
//! Execute separadamente, em release, com --ignored --nocapture --test-threads=1.
use std::{hint::black_box, path::Path, sync::Arc, time::Instant};

use filmcraft_engine::{FsServices, Session};
use filmcraft_export::{ExportSettings, Format, GpuRendering, HardwareEncoding, Progress};
use filmcraft_media::{Generator, MediaSource, generators::GeneratorSource};
use filmcraft_project::{
    EffectInstance, EffectKind, Interpolation, ItemId, ItemKind, Keyframe, Label, MediaClip, MediaRef, ParamKind, ParamValue, Project, SequenceSettings,
    TrackKind, Transition, TransitionAlign, TransitionId, effect_defs, find_effect,
};
use filmcraft_render::{
    RenderOptions, SourceProvider,
    plan::{FramePlan, PlanLayer, PlanStep},
};
use filmcraft_time::{FrameRate, Tick, TimeRange};
use serde_json::{Value, json};

use crate::preview::{Config, Job, measurement::Probe};

const RATE: FrameRate = FrameRate::FPS_30;
const SIZE: (u32, u32) = (96, 54);
const SAMPLES: [i64; 5] = [0, 6, 12, 18, 23];

#[derive(Clone)]
struct Case {
    id: String,
    kind: &'static str,
    effect: Option<EffectInstance>,
    speed: f64,
}

fn set(effect: &mut EffectInstance, id: &str, value: ParamValue) {
    effect.param_mut(id).unwrap_or_else(|| panic!("parâmetro inexistente: {id}")).value = value;
}

fn scalar(id: &str, param: &str, value: f64) -> EffectInstance {
    let mut effect = find_effect(id).unwrap().instance();
    set(&mut effect, param, ParamValue::Float(value));
    effect
}

/// Padrão não uniforme: rampas, cores saturadas e bordas. Nenhum arquivo de cliente.
fn fixture(path: &Path, reverse: bool, (w, h): (u32, u32)) {
    let image = image::RgbImage::from_fn(w, h, |x, y| {
        let x = if reverse { w - 1 - x } else { x };
        image::Rgb([(x * 220 / w + 16) as u8, (y * 220 / h + 16) as u8, if (x / 8 + y / 8) % 2 == 0 { 48 } else { 208 }])
    });
    image.save(path).unwrap();
}

fn media(project: &mut Project, path: &Path, session: &Session) -> ItemId {
    let source = session.media.probe_file(path.to_str().unwrap(), &*session.services).unwrap();
    project.add_item(
        "Padrão sintético",
        Label::Iris,
        ItemKind::Media(Box::new(MediaClip {
            media: MediaRef::File { path: path.to_string_lossy().into_owned() },
            info: source.info().clone(),
            interpret: Default::default(),
            mark_in: None,
            mark_out: None,
            markers: vec![],
            offline: false,
            proxy: None,
            identity: None,
        })),
        None,
    )
}

fn project(case: &Case, paths: [&Path; 2], size: (u32, u32), long: bool) -> (Session, ItemId) {
    let mut session = Session::new(Arc::new(FsServices));
    let mut p = Project::new("M3.0 sintético");
    let sources = paths.map(|path| media(&mut p, path, &session));
    let seq = p.new_sequence("M3.0", SequenceSettings { width: size.0, height: size.1, frame_rate: RATE, ..Default::default() }, 1, 1, None);
    let count = if long { 120 } else { 2 };
    let duration = if long { 900 } else { 12 };
    for index in 0..count {
        let mut item = p
            .make_track_item(
                sources[index % 2],
                TrackKind::Video,
                RATE.tick_of(index as i64 * duration),
                TimeRange::new(Tick::ZERO, RATE.tick_of(duration)),
                RATE,
            )
            .unwrap();
        item.speed = case.speed;
        let source_width = p.conformed_source_size(item.item, (1, 1)).unwrap().0;
        if let Some(motion) = item.effects.iter_mut().find(|e| e.effect == "motion") {
            set(motion, "scale", ParamValue::Float(size.0 as f64 / source_width * 100.0));
        }
        // A velocidade é observável em vídeo temporal; os demais casos usam padrões estáticos.
        if case.kind == "speed" {
            let g = GeneratorSource::new(Generator::CountingLeader, SIZE.0, SIZE.1, RATE, Tick::from_seconds_f64(10.0));
            let id = p.add_item(
                "Contagem sintética",
                Label::Iris,
                ItemKind::Media(Box::new(MediaClip {
                    media: MediaRef::Generator(g.generator.clone()),
                    info: g.info().clone(),
                    interpret: Default::default(),
                    mark_in: None,
                    mark_out: None,
                    markers: vec![],
                    offline: false,
                    proxy: None,
                    identity: None,
                })),
                None,
            );
            item.item = id;
            item.source_in = Tick::from_seconds_f64(1.0);
        }
        if let Some(effect) = &case.effect
            && case.kind != "transition"
        {
            if let Some(existing) = item.effects.iter_mut().find(|e| e.effect == effect.effect) {
                *existing = effect.clone();
            } else {
                item.effects.insert(0, effect.clone());
            }
        }
        p.sequence_mut(seq).unwrap().video_tracks[0].items.push(item);
    }
    if case.kind == "transition" || long {
        for index in 1..count {
            let track = &mut p.sequence_mut(seq).unwrap().video_tracks[0];
            track.transitions.push(Transition {
                id: TransitionId(10000 + index as u64),
                effect: if long { find_effect("cross_dissolve").unwrap().instance() } else { case.effect.clone().unwrap() },
                start: RATE.tick_of(index as i64 * duration - 6),
                duration: RATE.tick_of(12),
                from: Some(track.items[index - 1].id),
                to: Some(track.items[index].id),
                align: TransitionAlign::CenterAtCut,
                reverse: false,
            });
        }
    }
    p.sequence(seq).unwrap().check().unwrap();
    session.project = Arc::new(p);
    session.state.active_sequence = Some(seq);
    (session, seq)
}

fn changed(kind: &ParamKind, value: &ParamValue) -> Option<ParamValue> {
    match kind {
        ParamKind::Float { min, max, .. } => {
            let default = value.as_f64().unwrap_or(0.0);
            let delta = ((max - min) * 0.08).min(25.0);
            Some(ParamValue::Float(if default + delta <= *max { default + delta } else { default - delta }))
        }
        ParamKind::Choice(options) if options.len() > 1 => Some(ParamValue::Choice(1)),
        ParamKind::Bool => Some(ParamValue::Bool(!value.as_bool().unwrap_or(false))),
        ParamKind::Curve { hue } => {
            Some(ParamValue::Curve(if *hue { vec![[0.0, 0.6], [0.5, 0.7], [1.0, 0.6]] } else { vec![[0.0, 0.0], [0.5, 0.65], [1.0, 1.0]] }))
        }
        ParamKind::Wheel => Some(serde_json::from_value(json!({"Vec2":{"x":0.12,"y":-0.08}})).unwrap()),
        ParamKind::Color => Some(ParamValue::Color([0.25, 0.4, 0.65, 1.0])),
        _ => None,
    }
}

fn cases() -> (Vec<Case>, Vec<Value>) {
    let mut cases = vec![Case { id: "baseline".into(), kind: "baseline", effect: None, speed: 1.0 }];
    let mut excluded = Vec::new();
    for def in effect_defs().iter().filter(|d| matches!(d.kind, EffectKind::Video | EffectKind::VideoTransition)) {
        let reason = if def.category.iter().any(|s| matches!(*s, "Immersive Video" | "Text")) || def.id.starts_with("vr_") {
            Some("fora do escopo: VR ou texto")
        } else if def.category.iter().any(|s| matches!(*s, "Obsolete" | "Legacy")) {
            Some("catálogo legado/obsoleto: não propor para produto novo")
        } else if def.intrinsic || def.id.starts_with("graphic_") || def.id == "lumetri" {
            Some("medição específica, ou camada gráfica fora do escopo")
        } else {
            None
        };
        if let Some(reason) = reason {
            excluded.push(json!({"id":def.id,"reason":reason}));
            continue;
        }
        let mut effect = def.instance();
        if def.kind == EffectKind::Video
            && let Some((param, value)) = def.params.iter().find_map(|p| changed(&p.kind, &p.default).map(|v| (p.id, v)))
        {
            set(&mut effect, param, value);
        }
        cases.push(Case {
            id: def.id.into(),
            kind: if def.kind == EffectKind::VideoTransition { "transition" } else { "effect" },
            effect: Some(effect),
            speed: 1.0,
        });
    }
    let color = find_effect("lumetri").unwrap();
    for param in &color.params {
        if param.id.ends_with("_on") || param.id.starts_with("hdr_") || param.id == "curves_hdr_range" || param.id == "hsl_show_mask" {
            continue;
        }
        if let Some(value) = changed(&param.kind, &param.default) {
            let mut effect = color.instance();
            set(&mut effect, param.id, value);
            if param.id.starts_with("hsl_") {
                set(&mut effect, "hsl_on", ParamValue::Bool(true));
                if matches!(param.id, "hsl_hue" | "hsl_hue_range" | "hsl_sat_min" | "hsl_luma_min" | "hsl_luma_max" | "hsl_soft" | "hsl_denoise" | "hsl_blur") {
                    set(&mut effect, "hsl_sat", ParamValue::Float(150.0));
                }
            }
            if param.id.starts_with("vignette_") && param.id != "vignette_amount" {
                set(&mut effect, "vignette_amount", ParamValue::Float(-2.0));
            }
            if param.id == "look_intensity" {
                set(&mut effect, "look", ParamValue::Choice(1));
            }
            cases.push(Case { id: format!("color.{}", param.id), kind: "color", effect: Some(effect), speed: 1.0 });
        }
    }
    for (index, preset) in filmcraft_render::lumetri_presets::presets().iter().enumerate() {
        cases.push(Case { id: format!("preset.{index}"), kind: "preset", effect: Some(preset.instance()), speed: 1.0 });
    }
    for index in 1..=8 {
        let mut effect = color.instance();
        set(&mut effect, "look", ParamValue::Choice(index));
        cases.push(Case { id: format!("look.{index}"), kind: "look", effect: Some(effect), speed: 1.0 });
    }
    for interp in [Interpolation::Linear, Interpolation::EaseIn, Interpolation::EaseOut, Interpolation::Hold] {
        for (id, param, start, end) in [("motion", "scale", 100.0, 130.0), ("gaussian_blur", "blurriness", 0.0, 10.0)] {
            let mut effect = scalar(id, param, start);
            let mut key = Keyframe::new(Tick::ZERO, ParamValue::Float(start));
            key.interp = interp;
            effect.param_mut(param).unwrap().keyframes = vec![key, Keyframe::new(RATE.tick_of(9), ParamValue::Float(end))];
            cases.push(Case { id: format!("keyframe.{id}.{interp:?}"), kind: "keyframe", effect: Some(effect), speed: 1.0 });
        }
    }
    for speed in [0.25, 0.5, 1.0, 2.0, 4.0] {
        cases.push(Case { id: format!("speed.{speed}"), kind: "speed", effect: None, speed });
    }
    for (id, param, value) in [("motion", "scale", 125.0), ("motion", "rotation", 17.0), ("opacity", "opacity", 65.0)] {
        cases.push(Case { id: format!("transform.{param}"), kind: "transform", effect: Some(scalar(id, param, value)), speed: 1.0 });
    }
    for effect in [None, Some(scalar("sharpen", "amount", 15.0))] {
        cases.push(Case { id: format!("upscale.{}", if effect.is_some() { "sharpen" } else { "bilinear" }), kind: "upscale", effect, speed: 1.0 });
    }
    (cases, excluded)
}

fn job(session: &Session, seq: ItemId, frame: i64, size: (u32, u32)) -> Job {
    Job {
        project: session.project.clone(),
        sequence: seq,
        time: RATE.tick_of(frame),
        sources: session.media.provider(session.project.clone(), session.services.clone()),
        config: Config { max_width: size.0, max_height: size.1, quality: 80 },
        epoch: 0,
    }
}

/// Distribuição do maior erro RGB por pixel; alfa não participa (quadro sobre preto).
fn difference(a: &[u8], b: &[u8]) -> Value {
    assert_eq!(a.len(), b.len());
    assert!(!a.is_empty());
    let mut histogram = [0usize; 256];
    let mut total = 0u64;
    for (a, b) in a.as_chunks::<4>().0.iter().zip(b.as_chunks::<4>().0) {
        let delta = (0..3).map(|k| a[k].abs_diff(b[k])).max().unwrap();
        histogram[delta as usize] += 1;
        total += u64::from(delta);
    }
    let count = a.len() / 4;
    let mut sum = 0;
    let p99 = histogram
        .iter()
        .position(|n| {
            sum += n;
            sum > count * 99 / 100
        })
        .unwrap();
    let mean = total as f64 / count as f64;
    json!({"meanMaxRgb":mean,"p99":p99,"max":histogram.iter().rposition(|n| *n > 0).unwrap(),"pass":p99 <= 6 && mean < 1.5})
}

fn export(session: &Session, seq: ItemId, dir: &Path, gpu: bool, range: TimeRange, mp4: bool, size: (u32, u32)) -> (f64, Value) {
    let path = dir.join(if mp4 { "video.mp4" } else { "frame.png" });
    let mut settings = ExportSettings {
        path: path.to_string_lossy().into_owned(),
        format: if mp4 { Format::H264 } else { Format::PngSequence },
        include_audio: false,
        range: Some(range),
        frame_size: Some(size),
        frame_rate: Some(RATE),
        max_threads: if mp4 { 4 } else { 1 },
        gpu_rendering: if gpu { GpuRendering::Auto } else { GpuRendering::Off },
        hardware_encoding: if mp4 { HardwareEncoding::Auto } else { HardwareEncoding::Off },
        ..Default::default()
    };
    if mp4 {
        settings.bitrate_kbps = if size.0 >= 3840 { 45000 } else { 16000 };
    }
    let provider = session.media.provider(session.project.clone(), session.services.clone()).own_sources();
    let before = filmcraft_export::gpu_render_stats();
    let start = Instant::now();
    let result = filmcraft_export::export(&session.project, seq, &settings, &provider, &Progress::default()).unwrap();
    let seconds = start.elapsed().as_secs_f64();
    let after = filmcraft_export::gpu_render_stats();
    (seconds, json!({"gpuFrames":after.frames-before.frames,"fallbacks":after.fallbacks-before.fallbacks,"encoder":result.encoder,"seconds":seconds}))
}

fn png(dir: &Path, index: i64) -> Vec<u8> {
    let path = filmcraft_export::image_sequence_path(&dir.join("frame.png").to_string_lossy(), index as u64, 24);
    image::open(path).unwrap().to_rgba8().into_raw()
}

fn plan_details(plan: &FramePlan) -> Value {
    let mut layers = Vec::<&PlanLayer>::new();
    let mut gpu_transitions = 0;
    match plan {
        FramePlan::Layers { layers: list, .. } => layers.extend(list),
        FramePlan::Composite { steps, .. } => {
            for step in steps {
                match step {
                    PlanStep::Layers(list) => layers.extend(list),
                    PlanStep::Transition { inputs, .. } => {
                        gpu_transitions += 1;
                        for list in inputs {
                            layers.extend(list);
                        }
                    }
                    PlanStep::Adjust { .. } => {}
                }
            }
        }
        FramePlan::Image(_) => {}
    }
    json!({"kind":match plan { FramePlan::Image(_) => "cpuImage", FramePlan::Layers {..} => "layers", FramePlan::Composite {..} => "composite" },
        "gpuTransitions":gpu_transitions,"gpuEffectLayers":layers.iter().filter(|l| l.fx.is_some()).count(),
        "layerFormats":layers.iter().map(|l| l.frame.format_label()).collect::<Vec<_>>()})
}

fn measure_case(case: &Case, paths: [&Path; 2], dir: &Path, gpu: &mut Probe, cpu: &mut Probe) -> Value {
    let size = if case.kind == "upscale" { (SIZE.0 * 2, SIZE.1 * 2) } else { SIZE };
    let (session, seq) = project(case, paths, size, false);
    let mut session = session;
    if case.kind == "lut" {
        // Importa pela API do engine, inclusive parser e biblioteca do projeto.
        let cube = dir.join("fixture.cube");
        std::fs::write(&cube, "TITLE \"M3 sintético\"\nLUT_3D_SIZE 2\n0 0 0\n0.8 0.1 0\n0 0.9 0\n0.8 1 0\n0 0 0.8\n0.8 0.1 0.8\n0 0.9 0.8\n0.8 1 0.8\n")
            .unwrap();
        let imported = session.execute("lut.import", json!({"path":cube})).unwrap();
        let id = imported["id"].as_str().unwrap();
        let p = Arc::make_mut(&mut session.project);
        for clip in &mut p.sequence_mut(seq).unwrap().video_tracks[0].items {
            clip.effects.insert(0, {
                let mut e = find_effect("lumetri").unwrap().instance();
                set(&mut e, "input_lut", ParamValue::Text(format!("lib:{id}")));
                e
            });
        }
    }
    for on_gpu in [false, true] {
        std::fs::create_dir_all(dir.join(if on_gpu { "gpu" } else { "cpu" })).unwrap();
    }
    let range = TimeRange::new(Tick::ZERO, RATE.tick_of(24));
    let (_, cpu_export) = export(&session, seq, &dir.join("cpu"), false, range, false, size);
    let (_, gpu_export) = export(&session, seq, &dir.join("gpu"), true, range, false, size);
    let provider = session.media.provider(session.project.clone(), session.services.clone());
    let baseline = Case { id: "baseline".into(), kind: "baseline", effect: None, speed: 1.0 };
    let (base, base_seq) = project(&baseline, paths, size, false);
    let mut samples = Vec::new();
    for frame in SAMPLES {
        let job = job(&session, seq, frame, size);
        let (_, _, preview_gpu) = gpu.frame(&job);
        let (_, _, preview_cpu) = cpu.frame(&job);
        let exported_cpu = png(&dir.join("cpu"), frame);
        let exported_gpu = png(&dir.join("gpu"), frame);
        let payload = gpu.delivery(&job);
        let jpeg = image::load_from_memory(&payload[24..]).unwrap().to_rgba8().into_raw();
        let caps = gpu.caps();
        let plan = filmcraft_render::plan::plan_frame(&session.project, seq, job.time, RenderOptions::default(), caps, &provider);
        let (_, _, neutral) = cpu.frame(&self::job(&base, base_seq, frame, size));
        let speed_reference = if case.kind == "speed" {
            let clip = session.project.sequence(seq).unwrap().video_tracks[0].item_at(job.time).unwrap();
            let expected_time = Tick::from_seconds_f64(1.0 + (frame % 12) as f64 / 30.0 * case.speed);
            let expected = provider.source(clip.item).unwrap().video_frame(filmcraft_media::FrameRequest::full(expected_time)).unwrap().to_rgba8();
            json!({"sourceSeconds":expected_time.seconds(),"difference":difference(&preview_cpu,&expected)})
        } else {
            Value::Null
        };
        samples.push(json!({
            "frame":frame,"plan":plan_details(&plan),"speedReference":speed_reference,
            "previewGpuVsCpu":difference(&preview_gpu,&preview_cpu),
            "previewCpuVsExportCpu":difference(&preview_cpu,&exported_cpu),
            "previewGpuVsExportGpu":difference(&preview_gpu,&exported_gpu),
            "exportGpuVsCpu":difference(&exported_gpu,&exported_cpu),
            "jpegVsRaw":difference(&jpeg,&preview_gpu),"effectVsNeutral":difference(&preview_cpu,&neutral)
        }));
    }
    let pass = samples.iter().all(|s| {
        ["previewGpuVsCpu", "previewCpuVsExportCpu", "previewGpuVsExportGpu", "exportGpuVsCpu"].iter().all(|k| s[k]["pass"] == true)
            && (s["speedReference"].is_null() || s["speedReference"]["difference"]["pass"] == true)
    });
    let observable = samples.iter().any(|s| s["effectVsNeutral"]["max"].as_u64().unwrap() > 0);
    json!({"id":case.id,"kind":case.kind,"size":size,"effect":case.effect,"speed":case.speed,"pass":pass,"observable":observable,"samples":samples,"cpuExport":cpu_export,"gpuExport":gpu_export})
}

fn pitch_measurements(paths: [&Path; 2]) -> Vec<Value> {
    let mut rows = Vec::new();
    for speed in [0.25, 0.5, 1.0, 2.0, 4.0] {
        let case = Case { id: "tone".into(), kind: "audio", effect: None, speed };
        let (mut session, seq) = project(&case, paths, SIZE, false);
        let p = Arc::make_mut(&mut session.project);
        let g = GeneratorSource::new(Generator::Tone { hz: 440.0, db: -12.0 }, SIZE.0, SIZE.1, RATE, Tick::from_seconds_f64(10.0));
        let item = p.add_item(
            "Tom sintético",
            Label::Iris,
            ItemKind::Media(Box::new(MediaClip {
                media: MediaRef::Generator(g.generator.clone()),
                info: g.info().clone(),
                interpret: Default::default(),
                mark_in: None,
                mark_out: None,
                markers: vec![],
                offline: false,
                proxy: None,
                identity: None,
            })),
            None,
        );
        let mut clip = p.make_track_item(item, TrackKind::Audio, Tick::ZERO, TimeRange::new(Tick::ZERO, Tick::from_seconds_f64(2.0)), RATE).unwrap();
        clip.speed = speed;
        clip.maintain_pitch = true;
        p.sequence_mut(seq).unwrap().audio_tracks[0].items.push(clip);
        let provider = session.media.provider(session.project.clone(), session.services.clone());
        let sequence = session.project.sequence(seq).unwrap();
        let start = Instant::now();
        let audio = filmcraft_render::audio::mix_sequence(&session.project, sequence, 0, 48000, &provider);
        let seconds = start.elapsed().as_secs_f64();
        let samples = &audio.channels[0][4800..43200];
        let crossings = samples.windows(2).filter(|s| s[0] <= 0.0 && s[1] > 0.0).count();
        let hz = crossings as f64 / (samples.len() as f64 / 48000.0);
        rows.push(json!({"speed":speed,"maintainPitch":true,"expectedHz":440,"measuredHz":hz,"pass":(hz-440.0).abs()<5.0,"renderSeconds":seconds,
            "scope":"mixer offline; não mede saída cpal nem AAC"}));
    }
    rows
}

fn scopes(w: usize, h: usize, rgba: &[u8]) -> Value {
    let mut sample = Vec::new();
    let mut wave = Vec::new();
    let mut vector = Vec::new();
    for _ in 0..30 {
        let start = Instant::now();
        let signal = filmcraft_scopes::Signal::from_rgba8(w, h, black_box(rgba));
        sample.push(start.elapsed().as_secs_f64() * 1000.0);
        let params = filmcraft_scopes::Params::default();
        let start = Instant::now();
        black_box(filmcraft_scopes::waveform(&signal, filmcraft_scopes::WaveformType::Rgb, &params));
        wave.push(start.elapsed().as_secs_f64() * 1000.0);
        let start = Instant::now();
        black_box(filmcraft_scopes::vectorscope_yuv(&signal, &params));
        vector.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    json!({"signalMs":distribution(sample),"waveformMs":distribution(wave),"vectorscopeMs":distribution(vector)})
}

fn distribution(mut values: Vec<f64>) -> Value {
    values.sort_by(f64::total_cmp);
    json!({"n":values.len(),"median":values[values.len()/2],"p95":values[values.len()*95/100],"min":values[0],"max":values[values.len()-1]})
}

fn streaming(session: &Session, seq: ItemId, size: (u32, u32)) -> Value {
    use std::{
        sync::atomic::{AtomicU64, Ordering},
        time::Duration,
    };
    use tokio::sync::watch;
    tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap().block_on(async {
        let (sender, mut frames) = watch::channel(None);
        let renderer = crate::preview::Renderer::new(sender, Arc::new(AtomicU64::new(0)));
        assert!(renderer.gpu.is_some(), "GPU real na fila de prévia");
        renderer.submit(job(session, seq, 884, size));
        tokio::time::timeout(Duration::from_secs(10), frames.changed()).await.unwrap().unwrap();
        frames.borrow_and_update();
        let before = renderer.metrics.rendered.load(Ordering::Relaxed);
        let dropped = renderer.metrics.dropped.load(Ordering::Relaxed);
        let start = Instant::now();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        let mut clock = tokio::time::interval(Duration::from_secs_f64(1.0 / 30.0));
        clock.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut submitted = 0;
        let mut received = 0;
        loop {
            tokio::select! {
                biased;
                _ = tokio::time::sleep_until(deadline) => break,
                result = frames.changed() => {
                    result.unwrap();
                    frames.borrow_and_update();
                    received += 1;
                },
                _ = clock.tick(), if submitted < 60 => {
                    renderer.submit(job(session,seq,885+submitted,size));
                    submitted += 1;
                },
            }
        }
        let elapsed = start.elapsed().as_secs_f64();
        json!({"seconds":elapsed,"submitted":submitted,"received":received,"fps":received as f64/elapsed,
            "rendered":renderer.metrics.rendered.load(Ordering::Relaxed)-before,
            "dropped":renderer.metrics.dropped.load(Ordering::Relaxed)-dropped,"gpu":renderer.gpu,
            "scope":"fila real + JPEG + watch; sem WebSocket, desenho na tela ou áudio"})
    })
}

fn upscale_export(dir: &Path) -> Value {
    let paths = [dir.join("upscale-source-a.png"), dir.join("upscale-source-b.png")];
    fixture(&paths[0], false, (1920, 1080));
    fixture(&paths[1], true, (1920, 1080));
    let case = Case { id: "upscale-export".into(), kind: "color", effect: None, speed: 1.0 };
    let (session, seq) = project(&case, [&paths[0], &paths[1]], (1920, 1080), false);
    let mut results = Vec::new();
    let mut pixels = Vec::new();
    for gpu in [false, true] {
        let out = dir.join(if gpu { "upscale-export-gpu" } else { "upscale-export-cpu" });
        std::fs::create_dir_all(&out).unwrap();
        let (_, report) = export(&session, seq, &out, gpu, TimeRange::new(Tick::ZERO, RATE.tick_of(1)), false, (3840, 2160));
        pixels.push(png(&out, 0));
        results.push(report);
    }
    json!({"sourceSize":[1920,1080],"sequenceSize":[1920,1080],"outputSize":[3840,2160],"frames":1,"method":"engine-current-bilinear",
        "cpuExport":results[0],"gpuExport":results[1],"gpuVsCpu":difference(&pixels[0],&pixels[1]),
        "limitation":"PNG de um quadro; não aprova filtro de alta qualidade, nitidez de saída nem aceitação visual de halos"})
}

fn performance(dir: &Path, gpu: &mut Probe, cpu: &mut Probe) -> Vec<Value> {
    let case = Case { id: "hour".into(), kind: "color", effect: Some(scalar("lumetri", "contrast", 20.0)), speed: 1.0 };
    let mut rows = Vec::new();
    for size in [(1920, 1080), (3840, 2160)] {
        let fixtures = [dir.join(format!("native-{}-a.png", size.1)), dir.join(format!("native-{}-b.png", size.1))];
        fixture(&fixtures[0], false, size);
        fixture(&fixtures[1], true, size);
        let paths = [fixtures[0].as_path(), fixtures[1].as_path()];
        let (session, seq) = project(&case, paths, size, true);
        for (name, probe) in [("gpu", &mut *gpu), ("cpu", &mut *cpu)] {
            let mut raw = Vec::new();
            let mut delivery = Vec::new();
            // Aquecimento e amostras na transição de diferentes cortes, inclusive perto de 1 h.
            probe.delivery(&job(&session, seq, 900, size));
            for cut in [1, 30, 60, 119] {
                for offset in [-3, 0, 3] {
                    let job = job(&session, seq, cut * 900 + offset, size);
                    let start = Instant::now();
                    black_box(probe.frame(&job));
                    raw.push(start.elapsed().as_secs_f64() * 1000.0);
                    let start = Instant::now();
                    black_box(probe.delivery(&job));
                    delivery.push(start.elapsed().as_secs_f64() * 1000.0);
                }
            }
            let preview_fps = 12000.0 / delivery.iter().sum::<f64>();
            let (_, _, frame) = probe.frame(&job(&session, seq, 900, size));
            let scope = scopes(size.0 as usize, size.1 as usize, &frame);
            let out = dir.join(format!("perf-{}-{name}", size.1));
            std::fs::create_dir_all(&out).unwrap();
            let (_, export) = export(&session, seq, &out, name == "gpu", TimeRange::new(RATE.tick_of(885), RATE.tick_of(30)), true, size);
            let source = session.media.probe_file(out.join("video.mp4").to_str().unwrap(), &*session.services).unwrap();
            let mut encoded = Vec::new();
            for i in [0, 9, 15, 21, 29] {
                let actual = source.video_frame(filmcraft_media::FrameRequest::full(RATE.tick_of(i))).unwrap().to_rgba8();
                let (_, _, expected) = probe.frame(&job(&session, seq, 885 + i, size));
                encoded.push(json!({"frame":i,"difference":difference(&expected,&actual)}));
            }
            let streaming = if name == "gpu" { streaming(&session, seq, size) } else { Value::Null };
            rows.push(json!({"size":size,"path":name,"timelineSeconds":3600,"clips":120,"transitions":119,"colorOnEveryClip":true,"streaming":streaming,
                "previewRawMs":distribution(raw),"previewDeliveryMs":distribution(delivery),"previewFps":preview_fps,"scopes":scope,
                "exportRangeSeconds":1,"export":export,"mp4Comparisons":encoded}));
            eprintln!("M3.0 desempenho {}p {name}: {preview_fps:.2} qps", size.1);
        }
    }
    rows
}

#[test]
#[ignore = "medição física de GPU/exportação; executar isoladamente em release"]
fn m3_measurement() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = tempfile::Builder::new().prefix("m3-").tempdir_in(root.join("target")).unwrap();
    let paths = [dir.path().join("a.png"), dir.path().join("b.png")];
    fixture(&paths[0], false, SIZE);
    fixture(&paths[1], true, SIZE);
    let paths = [paths[0].as_path(), paths[1].as_path()];
    let mut gpu = Probe::new(true);
    let mut cpu = Probe::new(false);
    let capabilities = crate::encoding::Capabilities::detect();
    let (mut cases, excluded) = cases();
    cases.push(Case { id: "lut.cube".into(), kind: "lut", effect: None, speed: 1.0 });
    let report_path = root.join("target/m3.0-medicoes.json");
    let mut report = json!({"schema":1,"gpu":gpu.info(),"encoders":capabilities.names(),"profile":"release",
        "threshold":{"p99":6,"meanMaxRgbExclusive":1.5},"size":SIZE,"excluded":excluded,"cases":[],"performance":[],
        "catalog":effect_defs(),"presets":filmcraft_render::lumetri_presets::presets().iter().enumerate().map(|(i,p)| json!({"id":format!("preset.{i}"),"name":p.name,"folder":p.folder,"params":p.params})).collect::<Vec<_>>()});
    for case in cases {
        let folder = dir.path().join(&case.id);
        std::fs::create_dir_all(&folder).unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| measure_case(&case, paths, &folder, &mut gpu, &mut cpu)));
        let row = match result {
            Ok(row) => row,
            Err(_) => json!({"id":case.id,"kind":case.kind,"pass":false,"error":"falha durante a medição; consultar stderr"}),
        };
        eprintln!("M3.0 {}: pass={} observable={}", case.id, row["pass"], row["observable"]);
        report["cases"].as_array_mut().unwrap().push(row);
        std::fs::write(&report_path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    }
    report["pitch"] = json!(pitch_measurements(paths));
    report["upscaleExport2160"] = upscale_export(dir.path());
    report["performance"] = json!(performance(dir.path(), &mut gpu, &mut cpu));
    std::fs::write(&report_path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    assert!(report["cases"].as_array().unwrap().len() > 100, "o inventário precisa ser efetivamente exercitado");
    assert!(
        report["cases"].as_array().unwrap().iter().all(|row| row.get("error").is_none()),
        "falha de execução precisa ser investigada; divergências ficam no relatório"
    );
    eprintln!("M3.0 relatório: target/m3.0-medicoes.json");
}

#[test]
fn m3_difference_detects_localized_and_global_errors() {
    let a = vec![0; 400];
    let mut b = a.clone();
    b[0] = 255;
    assert_eq!(difference(&a, &b)["max"], 255);
    assert_eq!(difference(&a, &b)["pass"], false);
    assert_eq!(difference(&a, &a)["pass"], true);
}
