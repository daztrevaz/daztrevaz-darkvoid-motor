//! Aceites funcionais v3. Sem repetir a medição de paridade da M3.0.
use filmcraft_engine::{FsServices, Session};
use filmcraft_time::Tick;
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

fn session() -> Session {
    Session::new(Arc::new(FsServices))
}
fn clip(id: &str, media: &Path, start: f64, duration: f64) -> Value {
    json!({"id":id,"media":media,"track":"V1","start":start,"in":0,"out":duration,"volume":1,"fadeIn":0,"fadeOut":0})
}
fn input(clips: Vec<Value>) -> Value {
    json!({"format":"horizontal","fps":10,"clips":clips})
}
fn fixture(dir: &Path, reverse: bool) -> PathBuf {
    let path = dir.join(if reverse { "b.png" } else { "a.png" });
    image::RgbImage::from_fn(96, 54, |x, y| {
        let x = if reverse { 95 - x } else { x };
        image::Rgb([(16 + x * 2) as u8, (24 + y * 4) as u8, if (x / 8 + y / 8) % 2 == 0 { 40 } else { 210 }])
    })
    .save(&path)
    .unwrap();
    path
}
fn render(s: &Session, seconds: f64) -> Vec<u8> {
    let provider = s.media.provider(s.project.clone(), s.services.clone());
    filmcraft_render::render_sequence_rgba8(
        &s.project,
        s.state.active_sequence.unwrap(),
        Tick::from_seconds_f64(seconds),
        filmcraft_render::RenderOptions { scale: 0.05, ..Default::default() },
        &provider,
    )
    .2
}
fn transition_case(internal: &str) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (fixture(dir.path(), false), fixture(dir.path(), true));
    let mut s = session();
    let mut timeline = crate::timeline::Timeline::default();
    let a = clip("a", &a, 0.0, 2.0);
    let mut b = clip("b", &b, 2.0, 2.0);
    timeline.set(&mut s, input(vec![a.clone(), b.clone()])).unwrap();
    let times = [1.65, 1.85, 2.0, 2.15, 2.35];
    let baseline: Vec<_> = times.iter().map(|t| render(&s, *t)).collect();
    b["transitionIn"] = json!({"id":crate::catalog::public_transition(internal),"duration":1.0});
    timeline.set(&mut s, input(vec![a, b])).unwrap();
    assert!(times.iter().zip(baseline).any(|(t, plain)| render(&s, *t) != plain), "{internal} não muda a imagem");
    assert_eq!(s.active_sequence().unwrap().video_tracks[0].transitions.len(), 1);
}
macro_rules! transitions {
    ($($name:ident),+ $(,)?) => { $(#[test] fn $name() { transition_case(stringify!($name)); })+ };
}
transitions!(
    cross_dissolve,
    dip_to_black,
    dip_to_white,
    dip_to_color,
    blur_dissolve,
    additive_dissolve,
    film_dissolve,
    luma_fade,
    mosaic_transition,
    flip_motion,
    fold_motion,
    pop_motion,
    spin_motion,
    spring_motion,
    travel_motion,
    cross_zoom,
    zoom_blur,
    radial_blur,
    directional_blur_transition,
    lens_blur,
    flash,
    flare,
    glow,
    light_leak,
    light_sweep,
    chroma_leak,
    glitch,
    vhs_damage,
    tv_power,
    earthquake,
    flicker,
    glass,
    kaleidoscope,
    liquid_distortion,
    push,
    slide,
    split,
    whip,
    roll,
    stretch,
    spin_3d,
    page_peel,
    louver,
    slice,
    wave,
    linear_wipe,
    soft_wipe,
    radial_wipe,
    clock_wipe,
    star_wipe
);

#[test]
fn catalog_defaults_roundtrip_and_only_approved_items() {
    let catalog = crate::catalog::get();
    let items = catalog["items"].as_array().unwrap();
    assert_eq!(items.len(), 50);
    let mut ids = std::collections::HashSet::new();
    for item in items {
        assert!(ids.insert(item["id"].as_str().unwrap()));
        assert_eq!(item["kind"], "transition");
        assert_eq!(item["defaultDuration"], 1.0);
        let mut params = serde_json::Map::new();
        for param in item["params"].as_array().unwrap() {
            if !param["default"].is_null() {
                params.insert(param["id"].as_str().unwrap().into(), param["default"].clone());
            }
            assert!(!param["name"].as_str().unwrap().is_empty());
        }
        crate::catalog::transition(item["id"].as_str().unwrap(), &params, "teste").unwrap();
    }
}

#[test]
fn transitions_validate_atomically_and_rebuild_neighbors() {
    let dir = tempfile::tempdir().unwrap();
    let path = fixture(dir.path(), false);
    let a = clip("a", &path, 0.0, 2.0);
    let mut b = clip("b", &path, 2.0, 2.0);
    b["transitionIn"] = json!({"id":"transition.cross_dissolve","duration":1});
    let mut s = session();
    let mut timeline = crate::timeline::Timeline::default();
    timeline.set(&mut s, input(vec![a.clone(), b.clone()])).unwrap();
    let snapshot = s.project.clone();
    for bad in [
        json!({"id":"transition.morph_cut","duration":1}),
        json!({"id":"transition.cross_dissolve","duration":0}),
        json!({"id":"transition.cross_dissolve","duration":2.01}),
        json!({"id":"transition.cross_dissolve","duration":-1}),
        json!({"id":"transition.cross_dissolve","duration":1,"params":{"unknown":1}}),
        json!({"id":"transition.push","duration":1,"params":{"direction":"4"}}),
        json!({"id":"transition.push","duration":1,"params":{"motion_blur":101}}),
        json!({"id":"transition.push","duration":1,"params":{"motion_blur":true}}),
        json!({"id":"transition.flash","duration":1,"params":{"color":[1,0,0,2]}}),
        json!({"id":"transition.cross_zoom","duration":1,"params":{"center":[1]}}),
    ] {
        let mut bad_clip = b.clone();
        bad_clip["transitionIn"] = bad;
        let err = timeline.set(&mut s, input(vec![a.clone(), bad_clip])).unwrap_err();
        assert_eq!(err.code, "timeline_invalid");
        assert!(err.message.contains("b"));
        assert!(err.message.contains("transitionIn"));
        assert!(Arc::ptr_eq(&s.project, &snapshot));
    }
    for bad_a in [
        None,
        Some({
            let mut c = a.clone();
            c["out"] = json!(1.9);
            c
        }),
        Some({
            let mut c = a.clone();
            c["track"] = json!("V2");
            c
        }),
    ] {
        let mut clips = bad_a.into_iter().collect::<Vec<_>>();
        clips.push(b.clone());
        assert_eq!(timeline.set(&mut s, input(clips)).unwrap_err().code, "timeline_invalid");
    }
    let mut renamed = a;
    renamed["id"] = json!("novo");
    timeline.set(&mut s, input(vec![renamed.clone(), b.clone()])).unwrap();
    let track = &s.active_sequence().unwrap().video_tracks[0];
    assert_eq!(track.transitions[0].from, Some(track.items[0].id));
    b.as_object_mut().unwrap().remove("transitionIn");
    timeline.set(&mut s, input(vec![renamed, b])).unwrap();
    assert!(s.active_sequence().unwrap().video_tracks[0].transitions.is_empty());
}

#[test]
fn five_transitions_export_without_error() {
    let dir = tempfile::tempdir().unwrap();
    let paths = [fixture(dir.path(), false), fixture(dir.path(), true)];
    let names = ["cross_dissolve", "push", "flash", "linear_wipe", "glitch"];
    let clips = (0..6)
        .map(|i| {
            let mut c = clip(&format!("c{i}"), &paths[i % 2], i as f64 * 0.4, 0.4);
            if i > 0 {
                c["transitionIn"] = json!({"id":crate::catalog::public_transition(names[i-1]),"duration":0.2});
            }
            c
        })
        .collect();
    let mut s = session();
    crate::timeline::Timeline::default().set(&mut s, input(clips)).unwrap();
    let path = dir.path().join("transicoes.mp4");
    let settings = filmcraft_export::ExportSettings {
        path: path.to_string_lossy().into_owned(),
        frame_size: Some((96, 54)),
        frame_rate: Some(filmcraft_time::FrameRate::new(10, 1)),
        gpu_rendering: filmcraft_export::GpuRendering::Off,
        hardware_encoding: filmcraft_export::HardwareEncoding::Off,
        include_audio: true,
        max_threads: 2,
        ..Default::default()
    };
    let sources = s.media.provider(s.project.clone(), s.services.clone()).own_sources();
    filmcraft_export::export(&s.project, s.state.active_sequence.unwrap(), &settings, &sources, &Default::default()).unwrap();
    assert!(std::fs::metadata(path).unwrap().len() > 1000);
}

#[test]
fn centered_transition_uses_media_handles_and_repeats_file_edges() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/padrao.mp4");
    for (a_in, a_out, b_in, b_out) in [(0.5, 2.5, 1.0, 2.0), (1.0, 3.0, 0.0, 1.0)] {
        let mut a = clip("a", &path, 0.0, a_out);
        a["in"] = json!(a_in);
        let mut b = clip("b", &path, 2.0, b_out);
        b["in"] = json!(b_in);
        b["transitionIn"] = json!({"id":"transition.cross_dissolve","duration":0.8});
        let mut s = session();
        crate::timeline::Timeline::default().set(&mut s, input(vec![a, b])).unwrap();
        for time in [1.8, 2.2] {
            let actual = render(&s, time);
            let mut sides = Vec::new();
            for index in 0..2 {
                let mut reference = session();
                let mut project = (*s.project).clone();
                let sequence = s.state.active_sequence.unwrap();
                let track = &mut project.sequence_mut(sequence).unwrap().video_tracks[0];
                let mut item = track.items[index].clone();
                item.frame_hold = Some(item.source_time_at(Tick::from_seconds_f64(time)).clamp(Tick::ZERO, Tick::from_seconds_f64(3.0) - Tick(1)));
                item.start = Tick::ZERO;
                item.duration = Tick::from_seconds_f64(4.0);
                track.items = vec![item];
                track.transitions.clear();
                reference.project = Arc::new(project);
                reference.state.active_sequence = Some(sequence);
                sides.push(render(&reference, time));
            }
            let p = ((time - 1.6) / 0.8) as f32;
            let decode = |c: u8| {
                let c = c as f32 / 255.0;
                if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
            };
            let encode = |c: f32| (if c <= 0.0031308 { 12.92 * c } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }) * 255.0;
            for ((actual, a), b) in actual.chunks_exact(4).zip(sides[0].chunks_exact(4)).zip(sides[1].chunks_exact(4)) {
                for channel in 0..3 {
                    let expected = encode(decode(a[channel]) * (1.0 - p) + decode(b[channel]) * p);
                    assert!((actual[channel] as f32 - expected).abs() <= 2.0, "handles: {time}, canal {channel}");
                }
            }
        }
    }
}
