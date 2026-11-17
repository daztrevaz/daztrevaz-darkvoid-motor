//! Tradução do contrato v2 para a sequência e a álgebra de edição do engine.
use crate::protocol::{Error, Result, error};
use filmcraft_engine::{FsServices, Session};
use filmcraft_media::{MediaInfo, MediaKind};
use filmcraft_project::{ClipId, ItemId, Label, TrackItem, Transition, TransitionAlign, TransitionId};
use filmcraft_time::Tick;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::SystemTime,
};

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Clip {
    id: String,
    media: String,
    track: String,
    start: f64,
    #[serde(rename = "in")]
    source_in: f64,
    out: f64,
    volume: f64,
    fade_in: f64,
    fade_out: f64,
    #[serde(default)]
    muted: bool,
    #[serde(default)]
    transition_in: Option<TransitionInput>,
    #[serde(default)]
    color: Option<crate::color::Color>,
    #[serde(default)]
    look: Option<crate::color::Look>,
    #[serde(default)]
    effects: Vec<crate::effects::Effect>,
    #[serde(default)]
    transform: Option<crate::effects::Transform>,
    #[serde(default = "normal_speed")]
    speed: f64,
    #[serde(default)]
    gain: f64,
}
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct TransitionInput {
    id: String,
    duration: f64,
    #[serde(default)]
    params: serde_json::Map<String, Value>,
}
#[derive(Deserialize)]
struct Input {
    format: String,
    #[serde(default = "default_fps")]
    fps: f64,
    clips: Vec<Value>,
}
fn normal_speed() -> f64 {
    1.0
}
fn duration(c: &Clip) -> Tick {
    Tick(((Tick::from_seconds_f64(c.out) - Tick::from_seconds_f64(c.source_in)).0 as f64 / c.speed).round() as i64)
}
fn default_fps() -> f64 {
    30.0
}

#[derive(Clone)]
struct Media {
    stamp: (u64, Option<SystemTime>),
    info: MediaInfo,
    item: ItemId,
}
#[derive(Default)]
pub struct Timeline {
    format: String,
    fps: f64,
    clips: HashMap<String, (Clip, Vec<ClipId>)>,
    media: HashMap<String, Media>,
    lut_cache: crate::color::LutCache,
    color_effects: HashMap<String, Vec<filmcraft_project::EffectInstance>>,
}

fn invalid(id: &str, reason: &str) -> Error {
    error("timeline_invalid", format!("Clipe '{id}': {reason}"))
}
pub fn probe(session: &Session, path: &str) -> Result<MediaInfo> {
    if path.contains("://") || path.starts_with(r"\\") || path.starts_with("//") {
        return Err(crate::protocol::invalid());
    }
    if !std::path::Path::new(path).is_file() {
        return Err(error("media_not_found", format!("Arquivo não encontrado: '{path}'.")));
    }
    let source =
        session.media.probe_file(path, &*session.services).map_err(|_| error("unsupported_media", format!("Não foi possível abrir a mídia '{path}'.")))?;
    let info = source.info();
    if info.video.is_none() && !info.has_audio() {
        return Err(error("unsupported_media", format!("Formato de mídia não suportado: '{path}'.")));
    }
    Ok(info.clone())
}

impl Timeline {
    pub fn clip_audio(&self, id: &str) -> Result<(Vec<ClipId>, Tick, Tick)> {
        let (clip, ids) = self.clips.get(id).ok_or_else(|| invalid(id, "clipe não encontrado."))?;
        let audio = if clip.track.starts_with('V') { ids.get(1).copied().into_iter().collect() } else { ids.clone() };
        Ok((audio, Tick::from_seconds_f64(clip.start), duration(clip)))
    }
    pub fn is_active(&self) -> bool {
        !self.format.is_empty()
    }

    pub fn set(&mut self, session: &mut Session, params: Value) -> Result<bool> {
        let input: Input = serde_json::from_value(params).map_err(|_| invalid("linha do tempo", "formato, fps ou lista de clipes inválidos."))?;
        if !["vertical", "horizontal"].contains(&input.format.as_str()) || !input.fps.is_finite() || !(1.0..=120.0).contains(&input.fps) {
            return Err(invalid("linha do tempo", "formato ou fps fora do intervalo (1 a 120)."));
        }
        let mut clips = Vec::with_capacity(input.clips.len());
        let mut ids = HashSet::new();
        for value in input.clips {
            let id = value["id"].as_str().unwrap_or("sem id").to_owned();
            if let Some(transition) = value.get("transitionIn") {
                serde_json::from_value::<TransitionInput>(transition.clone()).map_err(|_| crate::catalog::invalid(&id, "transitionIn"))?;
            }
            for field in ["effects", "transform", "speed"] {
                let valid = match (field, value.get(field)) {
                    (_, None) => true,
                    ("effects", Some(v)) => serde_json::from_value::<Vec<crate::effects::Effect>>(v.clone()).is_ok(),
                    ("transform", Some(v)) => serde_json::from_value::<crate::effects::Transform>(v.clone()).is_ok(),
                    (_, Some(v)) => v.as_f64().is_some_and(|v| v.is_finite() && (0.25..=4.0).contains(&v)),
                };
                if !valid {
                    return Err(crate::catalog::invalid(&id, field));
                }
            }
            if let Some(color) = value.get("color") {
                serde_json::from_value::<crate::color::Color>(color.clone()).map_err(|_| crate::catalog::invalid(&id, "color"))?;
            }
            if let Some(look) = value.get("look") {
                serde_json::from_value::<crate::color::Look>(look.clone()).map_err(|_| crate::catalog::invalid(&id, "look"))?;
            }
            let c: Clip = serde_json::from_value(value).map_err(|_| invalid(&id, "campos ausentes ou com tipo inválido."))?;
            if c.id.trim().is_empty() || !ids.insert(c.id.clone()) {
                return Err(invalid(&c.id, "id vazio ou repetido."));
            }
            let duration = (c.out - c.source_in) / c.speed;
            if ![c.start, c.source_in, c.out, c.volume, c.fade_in, c.fade_out, c.gain].iter().all(|n| n.is_finite())
                || !(-24.0..=24.0).contains(&c.gain)
                || !c.speed.is_finite()
                || !(0.25..=4.0).contains(&c.speed)
                || Tick::from_seconds_f64(duration) <= Tick::ZERO
                || c.start < 0.0
                || c.source_in < 0.0
                || duration <= 0.0
                || c.start + duration > 3600.0
                || c.out > Tick(i64::MAX).seconds()
                || !(0.0..=2.0).contains(&c.volume)
                || c.fade_in < 0.0
                || c.fade_out < 0.0
                || c.fade_in > duration
                || c.fade_out > duration
                || Tick::from_seconds_f64(c.out) <= Tick::from_seconds_f64(c.source_in)
                || !["V1", "V2", "A1", "A2"].contains(&c.track.as_str())
            {
                return Err(invalid(&c.id, "trilha, tempos, volume ou fades fora do intervalo."));
            }
            clips.push(c);
        }
        // Valida todos os intervalos antes de abrir/importar arquivos ou alterar o projeto.
        clips.sort_by(|a, b| a.track.cmp(&b.track).then(a.start.total_cmp(&b.start)));
        for pair in clips.windows(2) {
            if pair[0].track == pair[1].track && Tick::from_seconds_f64(pair[0].start) + duration(&pair[0]) > Tick::from_seconds_f64(pair[1].start) {
                return Err(invalid(&pair[1].id, &format!("sobrepõe o clipe '{}' na trilha {}.", pair[0].id, pair[1].track)));
            }
        }
        for (index, c) in clips.iter().enumerate() {
            if let Some(tr) = &c.transition_in {
                let previous = index.checked_sub(1).map(|i| &clips[i]);
                let valid = previous.is_some_and(|p| {
                    p.track == c.track
                        && Tick::from_seconds_f64(p.start) + duration(p) == Tick::from_seconds_f64(c.start)
                        && tr.duration <= duration(p).seconds().min(duration(c).seconds())
                });
                if !c.track.starts_with('V') || !valid || !tr.duration.is_finite() || Tick::from_seconds_f64(tr.duration) <= Tick::ZERO {
                    return Err(crate::catalog::invalid(&c.id, "transitionIn.duration ou encontro com o clipe anterior"));
                }
                crate::catalog::transition(&tr.id, &tr.params, &c.id)?;
            }
        }
        let reset = self.format != input.format || self.fps != input.fps || session.active_sequence().is_none();
        let mut color_effects = HashMap::new();
        let mut visual_effects = HashMap::new();
        let mut luts = HashMap::new();
        for c in &clips {
            if !c.track.starts_with('V') && (c.color.is_some() || c.look.is_some() || !c.effects.is_empty() || c.transform.is_some()) {
                return Err(crate::catalog::invalid(&c.id, "color/look: use uma trilha de vídeo"));
            }
            color_effects.insert(c.id.clone(), self.lut_cache.prepare(c.color.as_ref(), c.look.as_ref(), &c.id, &mut luts)?);
            // Visual changes are already compared through Clip's finite public input. Engine
            // automatic coordinates contain NaN and must not invalidate every unchanged clip.
            visual_effects.insert(c.id.clone(), crate::effects::prepare(&c.effects, c.transform.as_ref(), &c.id)?);
        }
        let mut work = Session::new(Arc::new(FsServices));
        work.services = session.services.clone();
        if !reset {
            work.project = session.project.clone();
            work.state = session.state.clone();
            work.media = session.media.clone();
        }
        work.prefs.media.import_image_sequences = false;
        let mut library: Vec<_> = luts.into_values().collect();
        library.sort_by(|a, b| a.id.cmp(&b.id));
        library.dedup_by(|a, b| a.id == b.id);
        if work.project.luts != library {
            Arc::make_mut(&mut work.project).luts = library;
        }
        let mut media = if reset { HashMap::new() } else { self.media.clone() };
        let mut checked = HashSet::new();
        let mut refreshed = HashSet::new();
        for c in &clips {
            if checked.insert(c.media.clone()) {
                if c.media.contains("://") || c.media.starts_with(r"\\") || c.media.starts_with("//") {
                    return Err(invalid(&c.id, "selecione um arquivo local."));
                }
                let metadata =
                    std::fs::metadata(&c.media).map_err(|_| error("media_not_found", format!("Clipe '{}': arquivo não encontrado: '{}'.", c.id, c.media)))?;
                let stamp = (metadata.len(), metadata.modified().ok());
                if media.get(&c.media).is_none_or(|m| m.stamp != stamp) {
                    let info = probe(&work, &c.media).map_err(|e| error(e.code, format!("Clipe '{}': {}", c.id, e.message)))?;
                    media.insert(c.media.clone(), Media { stamp, info, item: ItemId(0) });
                    refreshed.insert(c.media.clone());
                }
            }
            let info = &media[&c.media].info;
            let image = info.kind == MediaKind::Still;
            if c.track.starts_with('V') != info.has_video() || (image && c.source_in != 0.0) || (!image && Tick::from_seconds_f64(c.out) > info.duration) {
                return Err(invalid(&c.id, "tipo incompatível com a trilha ou trecho além da duração da mídia."));
            }
        }
        for path in &refreshed {
            let imported = work.execute("file.import", json!({"paths":[path]})).map_err(engine_error)?;
            let item = imported["items"]
                .as_array()
                .and_then(|a| a.first())
                .and_then(Value::as_u64)
                .ok_or_else(|| error("unsupported_media", format!("Não foi possível importar a mídia '{path}'.")))?;
            if let Some(m) = media.get_mut(path) {
                m.item = ItemId(item);
            }
        }
        if reset {
            let (w, h) = if input.format == "vertical" { (1080, 1920) } else { (1920, 1080) };
            work.execute("file.newSequence", json!({"name":"Linha do tempo","width":w,"height":h,"fps":input.fps,"video":2,"audio":4}))
                .map_err(engine_error)?;
        }
        let changed: HashSet<_> = clips
            .iter()
            .filter(|c| {
                reset
                    || refreshed.contains(&c.media)
                    || self.clips.get(&c.id).is_none_or(|(old, _)| old != *c)
                    || self.color_effects.get(&c.id) != color_effects.get(&c.id)
            })
            .map(|c| c.id.clone())
            .collect();
        let removed: HashSet<_> =
            self.clips.iter().filter(|(id, _)| reset || changed.contains(*id) || !ids.contains(*id)).flat_map(|(_, (_, ids))| ids.iter().copied()).collect();
        let mut mapping = if reset { HashMap::new() } else { self.clips.clone() };
        mapping.retain(|id, _| ids.contains(id));
        if reset || !changed.is_empty() || !removed.is_empty() {
            work.edit_sequence("Atualizar linha do tempo", |seq, ctx, _| {
                for track in seq.all_tracks_mut() {
                    track.items.retain(|item| !removed.contains(&item.id));
                }
                let mut placements = Vec::new();
                for c in clips.iter().filter(|c| changed.contains(&c.id)) {
                    let old = if reset { None } else { self.clips.get(&c.id) };
                    let mut clip_ids = Vec::new();
                    let video = c.track.starts_with('V');
                    let index = usize::from(c.track.ends_with('2'));
                    let id = old.and_then(|(_, ids)| ids.first()).copied().unwrap_or_else(|| ClipId(ctx.alloc()));
                    let mut item = track_item(c, media[&c.media].item, id);
                    let track = if video { seq.video_tracks[index].id } else { seq.audio_tracks[index + 2].id };
                    item.enabled = video || (!c.muted && c.volume > 0.0);
                    clip_ids.push(id);
                    if video && media[&c.media].info.has_audio() {
                        let audio_id = old.and_then(|(_, ids)| ids.get(1)).copied().unwrap_or_else(|| ClipId(ctx.alloc()));
                        let mut audio = item.clone();
                        audio.id = audio_id;
                        audio.scale_to_frame = false;
                        audio.enabled = !c.muted && c.volume > 0.0;
                        placements.push((seq.audio_tracks[index].id, audio));
                        clip_ids.push(audio_id);
                    }
                    if video {
                        item.effects.extend(color_effects[&c.id].clone());
                        item.effects.extend(visual_effects[&c.id].clone());
                    }
                    placements.push((track, item));
                    mapping.insert(c.id.clone(), (c.clone(), clip_ids));
                }
                filmcraft_edit::overwrite(seq, placements, ctx)?;
                // Reconstrói os encontros após inserções, remoções e mudanças dos vizinhos.
                for track in seq.all_tracks_mut() {
                    track.transitions.clear();
                }
                for pair in clips.windows(2) {
                    let (previous, c) = (&pair[0], &pair[1]);
                    let Some(input) = &c.transition_in else { continue };
                    let index = usize::from(c.track.ends_with('2'));
                    let from = &mapping[&previous.id].1;
                    let to = &mapping[&c.id].1;
                    let duration = Tick::from_seconds_f64(input.duration);
                    let start = Tick::from_seconds_f64(c.start) - Tick(duration.0 / 2);
                    seq.video_tracks[index].transitions.push(Transition {
                        id: TransitionId(ctx.alloc()),
                        effect: crate::catalog::transition(&input.id, &input.params, &c.id).expect("parâmetros já validados"),
                        start,
                        duration,
                        from: Some(from[0]),
                        to: Some(to[0]),
                        align: TransitionAlign::CenterAtCut,
                        reverse: false,
                    });
                    // Um lado pode ser imagem ou vídeo sem áudio: nesse caso cruza com silêncio.
                    let a = from.get(1).copied();
                    let b = to.get(1).copied();
                    if a.is_some() || b.is_some() {
                        seq.audio_tracks[index].transitions.push(Transition {
                            id: TransitionId(ctx.alloc()),
                            effect: filmcraft_project::find_effect("constant_gain").expect("cruzamento de áudio").instance(),
                            start,
                            duration,
                            from: a,
                            to: b,
                            align: TransitionAlign::CenterAtCut,
                            reverse: false,
                        });
                    }
                }
                // As legendas não podem estender a duração definida pelos clipes.
                let end = clips.iter().map(|c| Tick::from_seconds_f64(c.start) + duration(c)).max().unwrap_or(Tick::ZERO);
                for track in &mut seq.caption_tracks {
                    track.captions.retain(|c| c.start < end);
                    for caption in &mut track.captions {
                        caption.duration = caption.duration.min(end - caption.start);
                    }
                    if let Some(animation) = &mut track.animation {
                        animation.cues.retain(|c| c.start < end);
                        for cue in &mut animation.cues {
                            cue.end = cue.end.min(end);
                        }
                    }
                }
                Ok(())
            })
            .map_err(engine_error)?;
            session.project = work.project;
            session.state = work.state;
            session.media = work.media;
        }
        self.format = input.format;
        self.fps = input.fps;
        self.clips = mapping;
        self.media = media;
        self.color_effects = color_effects;
        Ok(reset)
    }
}
fn engine_error(_: filmcraft_engine::EngineError) -> Error {
    error("engine_error", "Não foi possível montar a linha do tempo.")
}

fn track_item(c: &Clip, item: ItemId, id: ClipId) -> TrackItem {
    let mut volume = filmcraft_project::find_effect("volume").expect("efeito intrínseco de volume").instance();
    for (name, value) in [("fade_in_seconds", c.fade_in), ("fade_out_seconds", c.fade_out), ("declick_seconds", 0.003)] {
        volume.params.insert(name.into(), filmcraft_project::Param::new(filmcraft_project::ParamValue::Float(value)));
    }
    TrackItem {
        id,
        item,
        name: c.id.clone(),
        label: Label::Iris,
        custom_label_color: None,
        start: Tick::from_seconds_f64(c.start),
        source_in: Tick::from_seconds_f64(c.source_in),
        duration: duration(c),
        speed: c.speed,
        reverse: false,
        enabled: true,
        link: None,
        group: None,
        effects: vec![volume],
        markers: vec![],
        gain_db: c.gain + if c.volume > 0.0 { 20.0 * c.volume.log10() } else { -96.0 },
        frame_hold: None,
        scale_to_frame: c.track.starts_with('V'),
        essential: None,
        multicam: None,
        time_interpolation: Default::default(),
        maintain_pitch: true,
        hold_filters: false,
        field_options: None,
        source_channels: vec![],
        audio_stream: 0,
        graphic: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transition_audio_has_no_hole_at_cut_and_is_block_independent() {
        let video = fixture();
        let mut a = clip("a", &video, "V1", 0.0, 2.0);
        a["in"] = json!(0.5);
        a["out"] = json!(2.5);
        let mut b = clip("b", &video, "V1", 2.0, 2.5);
        b["in"] = json!(0.5);
        b["transitionIn"] = json!({"id":"transition.cross_dissolve","duration":1.0});
        let mut s = session();
        let mut timeline = Timeline::default();
        timeline.set(&mut s, input(vec![a.clone(), b.clone()])).unwrap();
        let source = s.media.source_for(&s.project, s.active_sequence().unwrap().video_tracks[0].items[0].item, &*s.services).unwrap();
        let snapshot = crate::mix::Snapshot::new(&s).unwrap();
        let read = snapshot.reader(48000);
        for sample in [84000i64, 95999, 96000, 108000] {
            let p = (sample as f32 - 72000.0) / 48000.0;
            let a = source.audio(sample + 24000, 1, 48000).unwrap();
            let b = source.audio(sample - 72000, 1, 48000).unwrap();
            let actual = read(sample, 1);
            for ch in 0..2 {
                let source_ch = ch.min(a.channels.len() - 1);
                assert!((actual[ch][0] - (a.channels[source_ch][0] * (1.0 - p) + b.channels[source_ch][0] * p)).abs() < 0.00001);
            }
        }
        let whole = read(72000, 48000);
        let split: Vec<_> = (0..48000).step_by(137).flat_map(|n| read(72000 + n as i64, (48000 - n).min(137))[0].clone()).collect();
        assert_eq!(whole[0], split);
        a["muted"] = json!(true);
        b["muted"] = json!(true);
        timeline.set(&mut s, input(vec![a, b])).unwrap();
        assert!(crate::mix::Snapshot::new(&s).unwrap().reader(48000)(72000, 48000).iter().flatten().all(|sample| *sample == 0.0));
    }
    fn session() -> Session {
        Session::new(Arc::new(FsServices))
    }
    fn constant_audio(path: &std::path::Path) {
        let samples = vec![0.25f32; 3 * 48000];
        let data: Vec<_> = samples.into_iter().flat_map(f32::to_le_bytes).collect();
        let mut wav = b"RIFF".to_vec();
        wav.extend((36 + data.len() as u32).to_le_bytes());
        wav.extend(b"WAVEfmt ");
        wav.extend(16u32.to_le_bytes());
        wav.extend(3u16.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(48000u32.to_le_bytes());
        wav.extend(192000u32.to_le_bytes());
        wav.extend(4u16.to_le_bytes());
        wav.extend(32u16.to_le_bytes());
        wav.extend(b"data");
        wav.extend((data.len() as u32).to_le_bytes());
        wav.extend(data);
        std::fs::write(path, wav).unwrap();
    }
    #[test]
    fn four_tracks_mix_linear_fades_gain_mute_and_snapshot_updates() {
        let dir = tempfile::tempdir().unwrap();
        let music = dir.path().join("music.wav");
        constant_audio(&music);
        let mut s = session();
        let mut timeline = Timeline::default();
        let mut v1 = clip("video1", &fixture(), "V1", 0.0, 2.0);
        v1["volume"] = json!(0.4);
        let mut v2 = v1.clone();
        v2["id"] = json!("video2");
        v2["track"] = json!("V2");
        let mut a1 = clip("musica", &music, "A1", 0.5, 2.25);
        a1["in"] = json!(0.25);
        a1["volume"] = json!(0.3);
        a1["fadeIn"] = json!(0.5);
        a1["fadeOut"] = json!(1.0);
        let mut a2 = clip("audio2", &music, "A2", 0.5, 2.0);
        a2["volume"] = json!(2.0);
        let mut params = input(vec![v1, v2, a1, a2]);
        timeline.set(&mut s, params.clone()).unwrap();
        let before = crate::mix::Snapshot::new(&s).unwrap();
        let read = before.reader(48000);
        let video = s.media.source_for(&s.project, s.active_sequence().unwrap().video_tracks[0].items[0].item, &*s.services).unwrap();
        for sample in [26400, 36000, 48000, 72000, 90000] {
            let time = sample as f64 / 48000.0;
            let expected_music = 0.25 * 0.3 * ((time - 0.5) / 0.5).min(1.0) * ((2.5 - 1.0 / 48000.0 - time) / 1.0).min(1.0);
            let original = video.audio(sample, 1, 48000).unwrap();
            let actual = read(sample, 1);
            for (channel, source) in actual.iter().zip(&original.channels) {
                assert!((channel[0] as f64 - (source[0] as f64 * 0.8 + expected_music + 0.5)).abs() < 0.00001);
            }
        }
        // O mesmo intervalo dividido em blocos preserva as amostras e as bordas silenciosas.
        let whole = read(0, 120000);
        let split: Vec<_> = (0..120000).step_by(137).flat_map(|start| read(start as i64, (120000 - start).min(137))[0].clone()).collect();
        assert_eq!(whole[0], split);
        assert_eq!(whole[0][0], 0.0);
        assert_eq!(whole[0][119999], 0.0);
        for c in params["clips"].as_array_mut().unwrap() {
            c["muted"] = json!(true);
        }
        timeline.set(&mut s, params.clone()).unwrap();
        assert!(crate::mix::Snapshot::new(&s).unwrap().reader(48000)(36000, 100)[0].iter().all(|v| *v == 0.0));
        assert_eq!(read(36000, 100)[0], whole[0][36000..36100], "snapshot anterior preservado");
        for c in params["clips"].as_array_mut().unwrap() {
            c["muted"] = json!(false);
            c["volume"] = json!(0.0);
        }
        timeline.set(&mut s, params).unwrap();
        assert!(crate::mix::Snapshot::new(&s).unwrap().reader(44100)(33075, 100)[0].iter().all(|v| *v == 0.0));
    }
    #[test]
    fn cuts_declick_and_overlapping_fades_are_sample_accurate() {
        let dir = tempfile::tempdir().unwrap();
        let media = dir.path().join("constant.wav");
        constant_audio(&media);
        let mut s = session();
        let mut timeline = Timeline::default();
        let mut a = clip("a", &media, "A1", 0.0, 1.0);
        a["volume"] = json!(2.0);
        let b = clip("b", &media, "A1", 1.0, 1.0);
        timeline.set(&mut s, input(vec![a.clone(), b])).unwrap();
        for rate in [16000, 44100, 48000] {
            let audio = crate::mix::Snapshot::new(&s).unwrap().reader(rate)(0, 2 * rate as usize);
            let maximum_jump = audio[0].windows(2).map(|s| (s[1] - s[0]).abs()).fold(0.0f32, f32::max);
            assert!(maximum_jump <= 0.5 / (rate as f32 * 0.003) + 0.00001);
            assert_eq!(audio[0][rate as usize - 1], 0.0);
            assert_eq!(audio[0][rate as usize], 0.0);
        }
        a["fadeIn"] = json!(1.0);
        a["fadeOut"] = json!(1.0);
        timeline.set(&mut s, input(vec![a])).unwrap();
        let midpoint = crate::mix::Snapshot::new(&s).unwrap().reader(48000)(24000, 1)[0][0];
        assert!((midpoint - 0.5 * 0.5 * (0.5 - 1.0 / 48000.0)).abs() < 0.000001);
    }
    fn clip(id: &str, media: &std::path::Path, track: &str, start: f64, end: f64) -> Value {
        json!({"id":id,"media":media,"track":track,"start":start,"in":0,"out":end,"volume":1,"fadeIn":0,"fadeOut":0})
    }
    fn input(clips: Vec<Value>) -> Value {
        json!({"format":"horizontal","clips":clips})
    }
    fn fixture() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/padrao.mp4")
    }
    fn still(dir: &std::path::Path, name: &str, color: [u8; 3], size: (u32, u32)) -> std::path::PathBuf {
        let path = dir.join(name);
        image::RgbImage::from_pixel(size.0, size.1, image::Rgb(color)).save(&path).unwrap();
        path
    }
    fn render(s: &Session, time: f64) -> Vec<u8> {
        let provider = s.media.provider(s.project.clone(), s.services.clone());
        filmcraft_render::render_sequence_rgba8(
            &s.project,
            s.state.active_sequence.unwrap(),
            Tick::from_seconds_f64(time),
            filmcraft_render::RenderOptions { scale: 0.05, ..Default::default() },
            &provider,
        )
        .2
    }
    #[test]
    fn validation_is_atomic_and_identifies_clip() {
        let mut s = session();
        let mut timeline = Timeline::default();
        let good = clip("primeiro", &fixture(), "V1", 0.0, 2.0);
        timeline.set(&mut s, input(vec![good.clone()])).unwrap();
        let snapshot = s.project.clone();
        for (field, value) in [
            ("in", json!(-1)),
            ("out", json!(0)),
            ("out", json!(10)),
            ("start", json!(-1)),
            ("volume", json!(3)),
            ("fadeIn", json!(3)),
            ("fadeOut", json!(-1)),
            ("track", json!("A1")),
            ("track", json!("V3")),
            ("out", Value::Null),
        ] {
            let mut bad = good.clone();
            bad[field] = value;
            let err = timeline.set(&mut s, input(vec![bad])).unwrap_err();
            assert_eq!(err.code, "timeline_invalid");
            assert!(err.message.contains("primeiro"));
            assert!(Arc::ptr_eq(&snapshot, &s.project));
        }
        let overlap = clip("segundo", &fixture(), "V1", 1.0, 2.0);
        assert!(timeline.set(&mut s, input(vec![good.clone(), overlap])).unwrap_err().message.contains("segundo"));
        assert!(timeline.set(&mut s, input(vec![good.clone(), good])).is_err());
        let missing = clip("sumiu", &fixture().with_extension("missing"), "V1", 0.0, 1.0);
        let e = timeline.set(&mut s, input(vec![missing])).unwrap_err();
        assert_eq!(e.code, "media_not_found");
        assert!(e.message.contains("sumiu") && e.message.contains("missing"));
        let dir = tempfile::tempdir().unwrap();
        let bad = dir.path().join("nao-midia.txt");
        std::fs::write(&bad, b"texto").unwrap();
        let e = timeline.set(&mut s, input(vec![clip("texto", &bad, "V1", 0.0, 1.0)])).unwrap_err();
        assert_eq!(e.code, "unsupported_media");
        assert!(e.message.contains("texto"));
        assert!(Arc::ptr_eq(&snapshot, &s.project));
    }
    #[test]
    fn gaps_still_images_and_v2_over_v1_use_engine_compositor() {
        let dir = tempfile::tempdir().unwrap();
        let red = still(dir.path(), "red.png", [255, 0, 0], (16, 9));
        let blue = still(dir.path(), "blue.png", [0, 0, 255], (16, 9));
        let mut s = session();
        let mut timeline = Timeline::default();
        timeline.set(&mut s, input(vec![clip("base", &red, "V1", 1.0, 4.0), clip("cima", &blue, "V2", 2.0, 1.0)])).unwrap();
        assert!(render(&s, 0.5).chunks_exact(4).all(|p| p[..3] == [0, 0, 0]));
        let a = render(&s, 1.0);
        let b = render(&s, 4.5);
        assert_eq!(a, b, "imagem permanece parada durante todo o clipe");
        // A filtragem nas bordas da imagem pequena pode amostrar o fundo preto.
        assert!(a.chunks_exact(4).filter(|p| p[0] > 250 && p[2] < 2).count() > a.len() / 5);
        let top = render(&s, 2.5);
        assert!(top.chunks_exact(4).filter(|p| p[2] > 250 && p[0] < 2).count() > top.len() / 5);
        assert_eq!(render(&s, 3.0), a, "intervalo semiaberto em V2");
        let sources = s.media.provider(s.project.clone(), s.services.clone());
        let silence = filmcraft_render::audio::mix_sequence(&s.project, s.active_sequence().unwrap(), 0, 48000, &sources);
        assert!(silence.channels.iter().flatten().all(|v| *v == 0.0));
        let portrait = still(dir.path(), "portrait.png", [0, 255, 0], (9, 16));
        timeline.set(&mut s, input(vec![clip("portrait", &portrait, "V1", 0.0, 1.0)])).unwrap();
        let px = render(&s, 0.5);
        assert_eq!(&px[..3], &[0, 0, 0], "encaixe sem cortar, com barras pretas");
        assert!(px.chunks_exact(4).any(|p| p[1] > 250));
        timeline.set(&mut s, input(vec![])).unwrap();
        assert_eq!(s.active_sequence().unwrap().duration(), Tick::ZERO);
        assert!(render(&s, 0.0).chunks_exact(4).all(|p| p[..3] == [0, 0, 0]));
    }
    #[test]
    fn video_audio_follows_source_in_and_gaps_are_silent() {
        let mut s = session();
        let mut timeline = Timeline::default();
        let mut a = clip("corte", &fixture(), "V1", 0.5, 1.25);
        a["in"] = json!(1.0);
        timeline.set(&mut s, input(vec![a])).unwrap();
        let seq = s.active_sequence().unwrap();
        let sources = s.media.provider(s.project.clone(), s.services.clone());
        for time in [0.0, 0.25, 0.75, 0.9] {
            let silence = filmcraft_render::audio::mix_sequence(&s.project, seq, Tick::from_seconds_f64(time).to_units_floor(48000), 480, &sources);
            assert!(silence.channels.iter().flatten().all(|v| *v == 0.0), "vazio no tempo {time}");
        }
        let actual = filmcraft_render::audio::mix_sequence(&s.project, seq, 28800, 480, &sources);
        let source = s.media.source_for(&s.project, seq.video_tracks[0].items[0].item, &*s.services).unwrap();
        let expected = source.audio(52800, 480, 48000).unwrap();
        assert!(actual.channels.iter().flatten().any(|v| v.abs() > 0.01));
        assert!(actual.channels[0].iter().zip(&expected.channels[0]).all(|(a, b)| (a - b).abs() < 0.00001));
    }
    #[test]
    fn partial_updates_keep_ids_and_share_one_source_and_reset_on_settings() {
        let mut s = session();
        let mut timeline = Timeline::default();
        let a = clip("a", &fixture(), "V1", 0.0, 1.0);
        let mut b = clip("b", &fixture(), "V1", 1.0, 1.0);
        timeline.set(&mut s, input(vec![a.clone(), b.clone()])).unwrap();
        let sequence = s.state.active_sequence;
        let original = s.active_sequence().unwrap().video_tracks[0].items.clone();
        let snap = s.project.clone();
        timeline.set(&mut s, input(vec![b.clone(), a.clone()])).unwrap();
        assert!(Arc::ptr_eq(&snap, &s.project), "envio idêntico não modifica o projeto");
        b["start"] = json!(2.0);
        timeline.set(&mut s, input(vec![a.clone(), b.clone()])).unwrap();
        assert_eq!(sequence, s.state.active_sequence);
        let items = &s.active_sequence().unwrap().video_tracks[0].items;
        assert_eq!(items[0], original[0]);
        assert_eq!(items[1].id, original[1].id);
        assert_eq!(items[0].item, items[1].item, "um decoder por arquivo");
        timeline.set(&mut s, input(vec![b])).unwrap();
        assert_eq!(s.active_sequence().unwrap().video_tracks[0].items.len(), 1);
        let mut params = input(vec![a]);
        params["fps"] = json!(25);
        assert!(timeline.set(&mut s, params).unwrap());
        assert_eq!(s.active_sequence().unwrap().settings.frame_rate.as_f64(), 25.0);
        let params = json!({"format":"vertical","clips":[]});
        assert!(timeline.set(&mut s, params).unwrap());
        assert_eq!(s.active_sequence().unwrap().settings.width, 1080);
    }
}
