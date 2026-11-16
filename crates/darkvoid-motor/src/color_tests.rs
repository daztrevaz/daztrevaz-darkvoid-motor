//! Aceites M3.2: grupos, mistura de looks, LUTs, osciloscópios e exportação completa de 1 h.
use filmcraft_engine::{FsServices, Session};
use filmcraft_time::{FrameRate, Tick};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};

fn session() -> Session {
    Session::new(Arc::new(FsServices))
}
fn fixture(path: &Path) {
    let img = filmcraft_render::lumetri_presets::preview_image(96, 54);
    image::RgbaImage::from_raw(96, 54, img.to_rgba8()).unwrap().save(path).unwrap();
}
fn clip(path: &Path) -> Value {
    json!({"id":"cor","media":path,"track":"V1","start":0,"in":0,"out":1,"volume":1,"fadeIn":0,"fadeOut":0})
}
fn set(timeline: &mut crate::timeline::Timeline, s: &mut Session, clip: Value) {
    timeline.set(s, json!({"format":"horizontal","fps":1,"clips":[clip]})).unwrap();
}
fn render(s: &Session) -> Vec<u8> {
    let provider = s.media.provider(s.project.clone(), s.services.clone());
    filmcraft_render::render_sequence_rgba8(
        &s.project,
        s.state.active_sequence.unwrap(),
        Tick::ZERO,
        filmcraft_render::RenderOptions { scale: 0.05, ..Default::default() },
        &provider,
    )
    .2
}
fn groups() -> [(&'static str, Value); 6] {
    [
        ("Básico", json!({"exposure":0.8})),
        ("Criativo", json!({"faded_film":45,"sharpen":35,"vibrance":40})),
        ("Curvas", json!({"curve_luma":[[0,0],[0.5,0.7],[1,1]]})),
        ("Rodas de cor", json!({"wheel_midtones":[0.3,-0.15]})),
        ("HSL", json!({"hsl_on":true,"hsl_hue":30,"hsl_hue_range":100,"hsl_sat":20})),
        ("Vinheta", json!({"vignette_amount":-3})),
    ]
}
fn export(s: &Session, path: &Path, format: filmcraft_export::Format) {
    let sources = s.media.provider(s.project.clone(), s.services.clone()).own_sources();
    let settings = filmcraft_export::ExportSettings {
        path: path.to_string_lossy().into_owned(),
        format,
        frame_size: Some((96, 54)),
        frame_rate: Some(FrameRate::new(1, 1)),
        include_audio: false,
        gpu_rendering: filmcraft_export::GpuRendering::Off,
        hardware_encoding: filmcraft_export::HardwareEncoding::Off,
        max_threads: 2,
        ..Default::default()
    };
    filmcraft_export::export(&s.project, s.state.active_sequence.unwrap(), &settings, &sources, &Default::default()).unwrap();
}

#[test]
fn all_six_groups_change_image_and_export_exact_preview_pixels() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fonte.png");
    fixture(&path);
    let mut s = session();
    let mut timeline = crate::timeline::Timeline::default();
    set(&mut timeline, &mut s, clip(&path));
    let baseline = render(&s);
    for (index, (group, params)) in groups().into_iter().enumerate() {
        let mut c = clip(&path);
        c["color"] = json!({"enabled":true,"params":params});
        set(&mut timeline, &mut s, c);
        let preview = render(&s);
        assert_ne!(preview, baseline, "{group}");
        let output = dir.path().join(format!("g{index}"));
        std::fs::create_dir(&output).unwrap();
        let target = output.join("quadro.png");
        export(&s, &target, filmcraft_export::Format::PngSequence);
        let frame = filmcraft_export::image_sequence_path(&target.to_string_lossy(), 0, 1);
        assert_eq!(image::open(frame).unwrap().to_rgba8().into_raw(), preview, "{group}");
    }
}

#[test]
fn all_26_looks_mix_zero_half_full_after_color() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fonte.png");
    fixture(&path);
    let mut s = session();
    let mut timeline = crate::timeline::Timeline::default();
    let mut base = clip(&path);
    base["color"] = json!({"enabled":true,"params":{"exposure":0.35}});
    set(&mut timeline, &mut s, base.clone());
    let baseline = render(&s);
    assert_eq!(crate::color::LOOKS.len(), 26);
    for (id, _, _, _) in crate::color::LOOKS {
        let mut results = Vec::new();
        for intensity in [0, 50, 100] {
            let mut c = base.clone();
            c["look"] = json!({"id":id,"intensity":intensity});
            set(&mut timeline, &mut s, c);
            results.push(render(&s));
            if intensity == 50 {
                let folder = dir.path().join(id);
                std::fs::create_dir(&folder).unwrap();
                let target = folder.join("look.png");
                export(&s, &target, filmcraft_export::Format::PngSequence);
                let frame = filmcraft_export::image_sequence_path(&target.to_string_lossy(), 0, 1);
                assert_eq!(image::open(frame).unwrap().to_rgba8().into_raw(), results[1], "exportação {id} a 50%");
            }
        }
        assert_eq!(results[0], baseline, "{id} a 0%");
        assert_ne!(results[2], baseline, "{id} a 100%");
        assert_ne!(results[1], results[0], "{id} a 50%");
        assert_ne!(results[1], results[2], "{id} a 50%");
        let mut c = base.clone();
        c["look"] = json!({"id":id});
        set(&mut timeline, &mut s, c);
        assert_eq!(render(&s), results[2], "{id}: intensidade ausente = 100%");
        for ((a, b), half) in results[0].chunks_exact(4).zip(results[2].chunks_exact(4)).zip(results[1].chunks_exact(4)) {
            for k in 0..3 {
                let linear = (filmcraft_color::srgb_to_linear(a[k] as f32 / 255.0) + filmcraft_color::srgb_to_linear(b[k] as f32 / 255.0)) * 0.5;
                let expected = filmcraft_color::linear_to_srgb(linear) * 255.0;
                assert!((expected - half[k] as f32).abs() <= 2.0, "{id}: mistura de 50%");
            }
        }
    }
}

fn cube(path: &Path, invert: bool) {
    let mut text = String::from("LUT_3D_SIZE 2\n");
    for b in 0..2 {
        for g in 0..2 {
            for r in 0..2 {
                text.push_str(&if invert { format!("{} {} {}\n", 1 - r, 1 - g, 1 - b) } else { format!("{r} {g} {b}\n") });
            }
        }
    }
    std::fs::write(path, text).unwrap();
}

#[test]
fn valid_cube_at_sixteen_mebibytes_reaches_the_renderer() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fonte.png");
    fixture(&path);
    let lut = dir.path().join("limite.cube");
    cube(&lut, true);
    let body = std::fs::read_to_string(&lut).unwrap();
    let text = format!("#{}\n{body}", " ".repeat(16 * 1024 * 1024 - body.len() - 2));
    assert_eq!(text.len(), 16 * 1024 * 1024);
    std::fs::write(&lut, text).unwrap();
    let mut s = session();
    let mut timeline = crate::timeline::Timeline::default();
    set(&mut timeline, &mut s, clip(&path));
    let baseline = render(&s);
    let mut c = clip(&path);
    c["color"] = json!({"enabled":true,"params":{"lut":lut}});
    set(&mut timeline, &mut s, c);
    assert_ne!(render(&s), baseline);
    assert_eq!(s.project.luts[0].text.len(), 16 * 1024 * 1024);
}

#[test]
fn lut_content_cache_reloads_changed_file_survives_delete_and_is_atomic() {
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join("fonte.png");
    fixture(&image);
    let a = dir.path().join("a.cube");
    let b = dir.path().join("b.cube");
    cube(&a, true);
    std::fs::copy(&a, &b).unwrap();
    let mut s = session();
    let mut timeline = crate::timeline::Timeline::default();
    set(&mut timeline, &mut s, clip(&image));
    let baseline = render(&s);
    let mut c = clip(&image);
    c["color"] = json!({"enabled":true,"params":{"lut":a}});
    set(&mut timeline, &mut s, c.clone());
    let graded = render(&s);
    assert_ne!(graded, baseline);
    let target = dir.path().join("lut.png");
    export(&s, &target, filmcraft_export::Format::PngSequence);
    let frame = filmcraft_export::image_sequence_path(&target.to_string_lossy(), 0, 1);
    assert_eq!(image::open(frame).unwrap().to_rgba8().into_raw(), graded);
    let first = s.project.luts[0].text.clone();
    c["color"]["params"]["lut"] = json!(b);
    set(&mut timeline, &mut s, c.clone());
    assert!(Arc::ptr_eq(&first, &s.project.luts[0].text));
    assert_eq!(render(&s), graded);
    let mut second = c.clone();
    second["id"] = json!("segundo");
    second["start"] = json!(1);
    second["color"]["params"]["lut"] = json!(a);
    timeline.set(&mut s, json!({"format":"horizontal","fps":1,"clips":[c.clone(),second]})).unwrap();
    assert_eq!(s.project.luts.len(), 1);
    cube(&b, false);
    set(&mut timeline, &mut s, c.clone());
    assert!(!Arc::ptr_eq(&first, &s.project.luts[0].text));
    assert_ne!(render(&s), graded);
    cube(&b, true);
    set(&mut timeline, &mut s, c.clone());
    let snapshot = s.project.clone();
    std::fs::remove_file(&b).unwrap();
    assert_eq!(render(&s), graded, "snapshot mantém conteúdo da LUT");
    assert_eq!(timeline.set(&mut s, json!({"format":"horizontal","clips":[c]})).unwrap_err().code, "media_not_found");
    assert!(Arc::ptr_eq(&snapshot, &s.project));
}

#[test]
fn invalid_luts_and_color_fields_are_rejected_before_project_changes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fonte.png");
    fixture(&path);
    let lut = dir.path().join("ruim.cube");
    let mut s = session();
    let mut timeline = crate::timeline::Timeline::default();
    set(&mut timeline, &mut s, clip(&path));
    let snapshot = s.project.clone();
    for text in [
        "LUT_3D_SIZE 1\n",
        "LUT_3D_SIZE 66\n",
        "LUT_3D_SIZE 2\n0 0 0\n",
        "LUT_1D_SIZE 2\nNaN 0 0\n1 1 1\n",
        "LUT_1D_SIZE 2\n0 0 0\n1 inf 1\n",
        "LUT_1D_SIZE 2\nDOMAIN_MAX inf 1 1\n0 0 0\n1 1 1\n",
    ] {
        std::fs::write(&lut, text).unwrap();
        let mut c = clip(&path);
        c["color"] = json!({"enabled":false,"params":{"lut":lut}});
        let err = timeline.set(&mut s, json!({"format":"horizontal","clips":[c]})).unwrap_err();
        assert_eq!(err.code, "lut_invalid");
        assert!(Arc::ptr_eq(&snapshot, &s.project));
    }
    std::fs::File::create(&lut).unwrap().set_len(16 * 1024 * 1024 + 1).unwrap();
    let mut c = clip(&path);
    c["color"] = json!({"enabled":true,"params":{"lut":lut}});
    assert_eq!(timeline.set(&mut s, json!({"format":"horizontal","clips":[c]})).unwrap_err().code, "lut_invalid");
    for (field, value) in [
        ("color", json!({"enabled":true,"params":{"unknown":1}})),
        ("color", json!({"enabled":true,"params":{"exposure":6}})),
        ("color", json!({"enabled":true,"params":{"exposure":true}})),
        ("color", json!({"enabled":true,"params":{"input_lut":"x"}})),
        ("color", json!({"enabled":true,"params":{"grade_mix":50}})),
        ("color", json!({"enabled":true,"params":{"wheel_midtones":[2,0]}})),
        ("color", json!({"enabled":true,"params":{"curve_luma":[[0,0],[0,1]]}})),
        ("color", json!({"enabled":true,"params":{"curve_luma":[[0.5,0],[0,1]]}})),
        ("color", json!({"enabled":true,"params":{"curve_luma":[[0,-1],[1,1]]}})),
        ("color", json!({"enabled":true,"params":{"hsl_luma_min":90,"hsl_luma_max":10}})),
        ("look", json!({"id":"look.night","intensity":101})),
        ("look", json!({"id":"look.night","intensity":-1})),
        ("look", json!({"id":"look.unknown"})),
    ] {
        let mut c = clip(&path);
        c[field] = value;
        let err = timeline.set(&mut s, json!({"format":"horizontal","clips":[c]})).unwrap_err();
        assert_eq!(err.code, "timeline_invalid");
        assert!(err.message.contains("cor") && err.message.contains(field));
        assert!(Arc::ptr_eq(&snapshot, &s.project));
    }
}

#[test]
fn catalog_has_six_groups_26_looks_and_valid_defaults() {
    let items = crate::color::items();
    assert_eq!(items.len(), 27);
    let params = items[0]["params"].as_array().unwrap();
    let groups: std::collections::HashSet<_> = params.iter().map(|p| p["group"].as_str().unwrap()).collect();
    assert_eq!(groups, std::collections::HashSet::from(["Básico", "Criativo", "Curvas", "Rodas de cor", "HSL", "Vinheta"]));
    let values = params.iter().map(|p| (p["id"].as_str().unwrap().into(), p["default"].clone())).collect();
    crate::color::LutCache::default().prepare(Some(&crate::color::Color { enabled: true, params: values }), None, "cor", &mut Default::default()).unwrap();
    assert_eq!(params.iter().find(|p| p["id"] == "lut").unwrap()["type"], "lut");
}

#[test]
fn scopes_are_png_from_current_graded_frame() {
    use base64::Engine;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fonte.png");
    fixture(&path);
    let mut s = session();
    let mut timeline = crate::timeline::Timeline::default();
    set(&mut timeline, &mut s, clip(&path));
    let params = json!({"kinds":["waveform","vectorscope"]});
    let before = crate::color::scopes(&s, Tick::ZERO, params.clone()).unwrap();
    let mut c = clip(&path);
    c["color"] = json!({"enabled":true,"params":{"saturation":0,"exposure":-1}});
    set(&mut timeline, &mut s, c);
    let after = crate::color::scopes(&s, Tick::ZERO, params).unwrap();
    for kind in ["waveform", "vectorscope"] {
        let png = base64::engine::general_purpose::STANDARD.decode(after[kind].as_str().unwrap()).unwrap();
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
        let image = image::load_from_memory(&png).unwrap();
        assert!(image.width() > 0 && image.height() > 0);
        assert_ne!(before[kind], after[kind]);
    }
    assert!(crate::color::scopes(&s, Tick::ZERO, json!({"kinds":["unknown"]})).is_err());
    assert_eq!(crate::color::scopes(&session(), Tick::ZERO, json!({"kinds":["waveform"]})).unwrap_err().code, "no_session");
}

#[test]
fn full_one_hour_with_color_on_every_clip_exports_without_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fonte.png");
    fixture(&path);
    let lut = dir.path().join("cor.cube");
    cube(&lut, true);
    let groups = groups();
    let clips: Vec<_> = (0..120)
        .map(|i| {
            let mut c = clip(&path);
            c["id"] = json!(format!("c{i}"));
            c["start"] = json!(i * 30);
            c["out"] = json!(30);
            c["color"] = json!({"enabled":true,"params":groups[i%6].1});
            c["look"] = json!({"id":crate::color::LOOKS[i%26].0,"intensity":50});
            if i % 12 == 0 {
                c["color"]["params"]["lut"] = json!(lut);
            }
            c
        })
        .collect();
    let mut s = session();
    crate::timeline::Timeline::default().set(&mut s, json!({"format":"horizontal","fps":1,"clips":clips})).unwrap();
    assert_eq!(s.active_sequence().unwrap().duration(), Tick::from_seconds_f64(3600.0));
    let target = dir.path().join("uma-hora.mp4");
    let start = std::time::Instant::now();
    export(&s, &target, filmcraft_export::Format::H264);
    let info = crate::timeline::probe(&s, target.to_str().unwrap()).unwrap();
    assert_eq!(info.duration, Tick::from_seconds_f64(3600.0));
    assert!(std::fs::metadata(target).unwrap().len() > 1000);
    eprintln!("M3.2: exportação completa de 3600 quadros, 96x54, 1 fps, 120 clipes, {:.3}s", start.elapsed().as_secs_f64());
}
