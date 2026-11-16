//! Aceites M3.3: caminho público, renderização, pontos-chave e paridade.
use filmcraft_engine::{FsServices, Session};
use filmcraft_time::Tick;
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};
static GPU_TEST: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn session() -> Session {
    Session::new(Arc::new(FsServices))
}
fn fixture(path: &Path) {
    image::RgbImage::from_fn(96, 54, |x, y| image::Rgb([(x * 2 + 16) as u8, (y * 4 + 24) as u8, if (x / 8 + y / 8) % 2 == 0 { 40 } else { 210 }]))
        .save(path)
        .unwrap();
}
fn clip(path: &Path) -> Value {
    json!({"id":"a","media":path,"track":"V1","start":0,"in":0,"out":1,"volume":1,"fadeIn":0,"fadeOut":0})
}
fn input(clips: Vec<Value>) -> Value {
    json!({"format":"horizontal","fps":10,"clips":clips})
}
fn render(s: &Session, t: f64) -> Vec<u8> {
    filmcraft_render::render_sequence_rgba8(
        &s.project,
        s.state.active_sequence.unwrap(),
        Tick::from_seconds_f64(t),
        filmcraft_render::RenderOptions { scale: 0.05, ..Default::default() },
        &s.media.provider(s.project.clone(), s.services.clone()),
    )
    .2
}
fn changed(p: &Value) -> Option<Value> {
    Some(match p["type"].as_str()? {
        "float" => {
            let d = p["default"].as_f64()?;
            let min = p["min"].as_f64()?;
            let max = p["max"].as_f64()?;
            let delta = ((max - min) * 0.08).min(25.0);
            json!(if d + delta <= max { d + delta } else { d - delta })
        }
        "bool" => json!(!p["default"].as_bool()?),
        "choice" => json!("1"),
        "color" => json!([0.25, 0.4, 0.65, 1]),
        "point" => json!([35, 20]),
        _ => return None,
    })
}
fn effect_case(id: &str) {
    let dir = tempfile::tempdir().unwrap();
    let mut path = dir.path().join("padrao.png");
    fixture(&path);
    if id == "posterize_time" {
        path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/padrao.mp4");
    }
    let mut s = session();
    let mut timeline = crate::timeline::Timeline::default();
    let c = clip(&path);
    timeline.set(&mut s, input(vec![c.clone()])).unwrap();
    let plain = render(&s, 0.3);
    let catalog = crate::catalog::get();
    let item = catalog["items"].as_array().unwrap().iter().find(|i| i["id"] == format!("effect.{id}")).unwrap();
    let params = item["params"].as_array().unwrap();
    let special = match id {
        "color_key" => json!({"tolerance":{"value":255}}),
        "color_replace" => json!({"similarity":{"value":100},"replace":{"value":[1,0,0,1]}}),
        "luma_key" => json!({"threshold":{"value":80},"cutoff":{"value":80}}),
        "posterize_time" => json!({"rate":{"value":1}}),
        _ => json!({}),
    };
    let mut cases = vec![special.as_object().unwrap().clone()];
    for p in params {
        if let Some(value) = changed(p) {
            let mut values = serde_json::Map::new();
            values.insert(p["id"].as_str().unwrap().into(), json!({"value":value}));
            cases.push(values);
        }
    }
    for values in cases {
        let mut c = c.clone();
        c["effects"] = json!([{"id":format!("effect.{id}"),"instanceId":"one","enabled":true,"params":values}]);
        timeline.set(&mut s, input(vec![c])).unwrap();
        if render(&s, 0.3) != plain {
            return;
        }
    }
    panic!("{id} não muda a imagem");
}

#[test]
fn catalog_has_61_effects_26_blends_and_roundtrips_defaults() {
    let cat = crate::catalog::get();
    let items = cat["items"].as_array().unwrap();
    assert_eq!(items.iter().filter(|i| i["kind"] == "effect").count(), 61);
    assert_eq!(crate::effects::BLENDS.len(), 26);
    for item in items.iter().filter(|i| i["kind"] == "effect") {
        let mut params = serde_json::Map::new();
        for p in item["params"].as_array().unwrap() {
            if !p["default"].is_null() {
                params.insert(p["id"].as_str().unwrap().into(), json!({"value":p["default"]}));
            }
        }
        let effects: Vec<crate::effects::Effect> =
            serde_json::from_value(json!([{"id":item["id"],"instanceId":"one","enabled":true,"params":params}])).unwrap();
        crate::effects::prepare(&effects, None, "a").unwrap();
    }
}

#[test]
#[ignore = "Aceite físico: exige GPU real"]
fn rotation_gpu_matches_cpu() {
    let _guard = GPU_TEST.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("padrao.png");
    fixture(&path);
    let mut gpu = filmcraft_gpu::export_renderer::ExportRenderer::new().expect("GPU real para aceitar rotação");
    let mut s = session();
    let mut timeline = crate::timeline::Timeline::default();
    for angle in [-90.0, -37.0, 19.0, 90.0, 180.0] {
        let mut c = clip(&path);
        c["transform"] = json!({"rotation":{"value":angle},"scale":{"value":80}});
        timeline.set(&mut s, input(vec![c])).unwrap();
        let sources = s.media.provider(s.project.clone(), s.services.clone());
        let opts = filmcraft_render::RenderOptions { scale: 0.05, ..Default::default() };
        let cpu = render(&s, 0.3);
        let actual = gpu.render(&s.project, s.state.active_sequence.unwrap(), Tick::from_seconds_f64(0.3), opts, &sources).unwrap().over_black_rgba8();
        let mut diffs: Vec<_> =
            cpu.chunks_exact(4).zip(actual.chunks_exact(4)).map(|(a, b)| (0..3).map(|i| a[i].abs_diff(b[i]) as u32).max().unwrap()).collect();
        diffs.sort_unstable();
        let mean = diffs.iter().sum::<u32>() as f64 / diffs.len() as f64;
        let p99 = diffs[diffs.len() * 99 / 100];
        eprintln!("rotação {angle}: média {mean:.4}, p99 {p99}");
        assert!(mean < 1.5 && p99 <= 6);
    }
}

#[test]
fn animated_zoom_blur_and_chroma_export_match_preview() {
    let _guard = GPU_TEST.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("padrao.png");
    fixture(&path);
    let key =
        |a: Value, b: Value| json!({"value":a,"keyframes":[{"time":0,"value":a,"interpolation":"linear"},{"time":0.8,"value":b,"interpolation":"easeOut"}]});
    let video = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/padrao.mp4");
    let mut gpu = filmcraft_gpu::export_renderer::ExportRenderer::new();
    for (name, visual) in [
        ("zoom", json!({"transform":{"scale":key(json!(100),json!(130))}})),
        ("blur", json!({"effects":[{"id":"effect.gaussian_blur","instanceId":"blur","enabled":true,"params":{"blurriness":key(json!(0),json!(12))}}]})),
        ("chroma", json!({"effects":[{"id":"effect.ultra_key","instanceId":"key","enabled":true,"params":{"key_color":{"value":[0,1,0,1]}}}]})),
    ] {
        let mut s = session();
        let mut c = clip(if name == "zoom" { &path } else { &video });
        for (k, v) in visual.as_object().unwrap() {
            c[k] = v.clone();
        }
        crate::timeline::Timeline::default().set(&mut s, input(vec![c])).unwrap();
        let target = dir.path().join(format!("{name}.png"));
        let sources = s.media.provider(s.project.clone(), s.services.clone()).own_sources();
        let settings = filmcraft_export::ExportSettings {
            path: target.to_string_lossy().into_owned(),
            format: filmcraft_export::Format::PngSequence,
            frame_size: Some((96, 54)),
            frame_rate: Some(filmcraft_time::FrameRate::new(10, 1)),
            include_audio: false,
            alpha: false,
            gpu_rendering: filmcraft_export::GpuRendering::Off,
            hardware_encoding: filmcraft_export::HardwareEncoding::Off,
            max_threads: 2,
            ..Default::default()
        };
        filmcraft_export::export(&s.project, s.state.active_sequence.unwrap(), &settings, &sources, &Default::default()).unwrap();
        for n in [0, 3, 7] {
            let cpu = render(&s, n as f64 / 10.0);
            let frame = filmcraft_export::image_sequence_path(&target.to_string_lossy(), n, 10);
            let encoded = image::open(frame).unwrap().to_rgba8().into_raw();
            let max_rgb = encoded.chunks_exact(4).zip(cpu.chunks_exact(4)).flat_map(|(a, b)| (0..3).map(move |k| a[k].abs_diff(b[k]))).max().unwrap();
            assert_eq!(max_rgb, 0, "{name} quadro {n}");
            let Some(gpu) = gpu.as_mut() else { continue };
            let actual = gpu
                .render(
                    &s.project,
                    s.state.active_sequence.unwrap(),
                    Tick::from_seconds_f64(n as f64 / 10.0),
                    filmcraft_render::RenderOptions { scale: 0.05, ..Default::default() },
                    &sources,
                )
                .unwrap()
                .over_black_rgba8();
            let mut diff: Vec<_> =
                cpu.chunks_exact(4).zip(actual.chunks_exact(4)).map(|(a, b)| (0..3).map(|k| a[k].abs_diff(b[k]) as u32).max().unwrap()).collect();
            diff.sort_unstable();
            eprintln!("{name} {n}: p99 {}, média {:.3}", diff[diff.len() * 99 / 100], diff.iter().sum::<u32>() as f64 / diff.len() as f64);
            if diff[diff.len() * 99 / 100] > 6 {
                image::save_buffer(concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/m33-preview-cpu.png"), &cpu, 96, 54, image::ColorType::Rgba8).unwrap();
                image::save_buffer(concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/m33-preview-gpu.png"), &actual, 96, 54, image::ColorType::Rgba8).unwrap();
            }
            assert!(diff[diff.len() * 99 / 100] <= 6, "{name} quadro {n}");
        }
    }
}

#[test]
fn keyframes_keep_media_time_and_validate_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("padrao.png");
    fixture(&path);
    let mut s = session();
    let mut timeline = crate::timeline::Timeline::default();
    for interpolation in ["linear", "easeIn", "easeOut", "hold"] {
        let mut c = clip(&path);
        c["transform"] =
            json!({"scale":{"value":50,"keyframes":[{"time":0.2,"value":80,"interpolation":interpolation},{"time":0.8,"value":120,"interpolation":"linear"}]}});
        timeline.set(&mut s, input(vec![c.clone()])).unwrap();
        let scale = s.active_sequence().unwrap().video_tracks[0].items[0].effect("motion").unwrap().param("scale").unwrap();
        assert_eq!(scale.f64_at(Tick::ZERO), 80.0);
        assert_eq!(scale.f64_at(Tick::from_seconds_f64(1.0)), 120.0);
        let mid = scale.f64_at(Tick::from_seconds_f64(0.5));
        match interpolation {
            "linear" => assert!((mid - 100.0).abs() < 0.001),
            "hold" => assert_eq!(mid, 80.0),
            _ => assert!(mid > 80.0 && mid < 120.0),
        }
        let snapshot = s.project.clone();
        for bad in [json!(-1), json!(0.2), json!(0.1)] {
            let mut bad_clip = c.clone();
            bad_clip["transform"]["scale"]["keyframes"][1]["time"] = bad;
            assert_eq!(timeline.set(&mut s, input(vec![bad_clip])).unwrap_err().code, "timeline_invalid");
            assert!(Arc::ptr_eq(&snapshot, &s.project));
        }
        for bad in [json!({"position":{"value":[1]}}), json!({"opacity":{"value":101}}), json!({"blendMode":"invalid"}), json!({"unknown":1})] {
            let mut bad_clip = c.clone();
            bad_clip["transform"] = bad;
            assert_eq!(timeline.set(&mut s, input(vec![bad_clip])).unwrap_err().code, "timeline_invalid");
            assert!(Arc::ptr_eq(&snapshot, &s.project));
        }
    }
    let video = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/padrao.mp4");
    let mut c = clip(&video);
    c["in"] = json!(1);
    c["out"] = json!(2);
    c["speed"] = json!(2);
    c["transform"] = json!({"scale":{"value":50,"keyframes":[{"time":1,"value":80,"interpolation":"linear"},{"time":2,"value":120,"interpolation":"hold"}]}});
    timeline.set(&mut s, input(vec![c])).unwrap();
    let item = &s.active_sequence().unwrap().video_tracks[0].items[0];
    let t = item.effect_time_at(Tick::from_seconds_f64(0.25));
    assert_eq!(t, Tick::from_seconds_f64(1.5));
    assert_eq!(item.effect("motion").unwrap().f64_at("scale", t), 100.0);
}

#[test]
fn speed_updates_duration_overlap_and_preserves_audio_pitch() {
    let video = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/padrao.mp4");
    let mut s = session();
    let mut timeline = crate::timeline::Timeline::default();
    for speed in [0.25, 0.5, 1.0, 2.0, 4.0] {
        let mut c = clip(&video);
        c["out"] = json!(3);
        c["speed"] = json!(speed);
        timeline.set(&mut s, input(vec![c.clone()])).unwrap();
        assert_eq!(s.active_sequence().unwrap().duration(), Tick::from_seconds_f64(3.0 / speed));
        let snapshot = crate::mix::Snapshot::new(&s).unwrap();
        let read = snapshot.reader(48000);
        let audio = read(4800, 24000);
        let crossings = audio[0].windows(2).filter(|p| p[0] <= 0.0 && p[1] > 0.0).count();
        let hz = crossings as f64 * 2.0;
        assert!((hz - 440.0).abs() < 12.0, "{speed}: {hz} Hz");
        let mut next = c.clone();
        next["id"] = json!("b");
        next["start"] = json!(3.0 / speed);
        timeline.set(&mut s, input(vec![c.clone(), next.clone()])).unwrap();
        next["start"] = json!(3.0 / speed - 0.1);
        assert_eq!(timeline.set(&mut s, input(vec![c, next])).unwrap_err().code, "timeline_invalid");
    }
}

#[test]
fn blend_crop_effect_order_and_disabled_instances() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("padrao.png");
    fixture(&path);
    let mut s = session();
    let mut timeline = crate::timeline::Timeline::default();
    let base = clip(&path);
    timeline.set(&mut s, input(vec![base.clone()])).unwrap();
    let plain = render(&s, 0.3);
    for (n, (mode, _)) in crate::effects::BLENDS.iter().enumerate() {
        let mut c = base.clone();
        c["track"] = json!("V2");
        c["id"] = json!("b");
        c["transform"] = json!({"blendMode":mode});
        timeline.set(&mut s, input(vec![base.clone(), c])).unwrap();
        let effect = s.active_sequence().unwrap().video_tracks[1].items[0].effect("opacity").unwrap();
        assert_eq!(effect.param("blend").unwrap().value, filmcraft_project::ParamValue::Choice(if n == 0 { 0 } else { n as u32 + 1 }));
        assert!(!render(&s, 0.3).is_empty());
    }
    let mut c = base.clone();
    c["transform"] = json!({"crop":{"left":{"value":25},"top":{"value":10},"right":{"value":15},"bottom":{"value":20}},"position":{"value":[960,540]},"opacity":{"value":80}});
    timeline.set(&mut s, input(vec![c])).unwrap();
    let cropped = render(&s, 0.3);
    assert_ne!(cropped, plain);
    assert_eq!(&cropped[..3], &[0, 0, 0]);
    let invert = json!({"id":"effect.invert","instanceId":"one","enabled":true,"params":{}});
    let mut c = base;
    c["effects"] = json!([invert.clone(), invert]);
    c["effects"][1]["instanceId"] = json!("two");
    c["effects"][1]["enabled"] = json!(false);
    timeline.set(&mut s, input(vec![c.clone()])).unwrap();
    assert_ne!(render(&s, 0.3), plain);
    c["effects"][1]["enabled"] = json!(true);
    timeline.set(&mut s, input(vec![c.clone()])).unwrap();
    let twice = render(&s, 0.3);
    assert!(twice.iter().zip(&plain).all(|(a, b)| a.abs_diff(*b) <= 1));
    let snapshot = s.project.clone();
    for bad in [
        json!([{"id":"effect.posterize","instanceId":"x","enabled":false,"params":{}}]),
        json!([{"id":"effect.invert","instanceId":"x","enabled":true,"params":{"unknown":{"value":1}}}]),
        json!([{"id":"effect.gaussian_blur","instanceId":"x","enabled":true,"params":{"blurriness":{"value":-1}}}]),
    ] {
        c["effects"] = bad;
        assert_eq!(timeline.set(&mut s, input(vec![c.clone()])).unwrap_err().code, "timeline_invalid");
        assert!(Arc::ptr_eq(&snapshot, &s.project));
    }
}

macro_rules! effects { ($($id:ident),+ $(,)?) => { $(#[test] fn $id() { effect_case(stringify!($id)); })+ }; }
effects!(
    gaussian_blur,
    directional_blur,
    bokeh_blur,
    focus_blur,
    compound_blur,
    channel_blur,
    sharpen,
    unsharp_mask,
    lens_distortion,
    spherize,
    twirl,
    wave_warp,
    turbulent_displace,
    magnify,
    mirror,
    offset,
    corner_pin,
    camera_shake,
    wiggle,
    strobe,
    posterize_time,
    basic_3d,
    rotate_3d,
    lens_flare,
    light_leaks,
    glint,
    edge_glow,
    wonder_glow,
    volumetric_rays,
    lighting_effects,
    echo_glow,
    black_white,
    invert,
    find_edges,
    color_emboss,
    mosaic,
    noise,
    rgb_split,
    brush_strokes,
    roughen_edges,
    stroke,
    drop_shadow,
    long_shadow,
    vignette,
    tint,
    color_pass,
    color_replace,
    levels,
    channel_mix,
    brightness_contrast,
    four_color_gradient,
    gradient,
    horizontal_flip,
    vertical_flip,
    rounded_crop,
    warp_stabilizer,
    ultra_key,
    color_key,
    luma_key,
    track_matte,
    alpha_adjust
);
