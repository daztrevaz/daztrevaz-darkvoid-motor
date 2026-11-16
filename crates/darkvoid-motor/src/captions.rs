//! Protocol v1 captions; validate the whole replacement before changing the session.
use crate::protocol::{Result, error};
use filmcraft_engine::Session;
use filmcraft_project::{
    Caption, CaptionFormat, CaptionTrack, ClipId, TrackId,
    caption_animation::{AnimatedCaptionStyle, AnimatedCaptions, AnimatedCue, CaptionWord},
};
use filmcraft_time::Tick;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    io::Write,
    path::Path,
    sync::{Arc, OnceLock},
};

pub fn fonts() -> &'static Vec<String> {
    static FONTS: OnceLock<Vec<String>> = OnceLock::new();
    FONTS.get_or_init(|| {
        filmcraft_text::fonts::scan_system();
        let mut names: Vec<_> = filmcraft_text::fonts::all_faces()
            .iter()
            .filter(|f| f.info.origin == "system" || f.info.family == "Inter")
            .map(|f| f.info.family.clone())
            .collect();
        names.sort();
        names.dedup();
        names
    })
}
fn invalid() -> crate::protocol::Error {
    error("captions_invalid", "Dados de legenda inválidos. Confira os tempos e o estilo.")
}
fn no_session() -> crate::protocol::Error {
    error("no_session", "Abra um vídeo antes de usar as legendas.")
}
#[derive(Deserialize)]
struct Set {
    cues: Vec<Cue>,
    style: AnimatedCaptionStyle,
}
#[derive(Deserialize)]
struct Cue {
    start: f64,
    end: f64,
    words: Vec<Word>,
}
#[derive(Deserialize)]
struct Word {
    text: String,
    start: f64,
    end: f64,
}
#[derive(Deserialize)]
pub struct Output {
    path: String,
}
fn bounded(v: f32, min: f32, max: f32) -> bool {
    v.is_finite() && (min..=max).contains(&v)
}
fn valid_color(s: &str) -> bool {
    filmcraft_captions::animated::color(s, 1.0).is_some()
}
fn valid_style(s: &AnimatedCaptionStyle) -> bool {
    (100..=900).contains(&s.weight)
        && bounded(s.size, 1.0, 256.0)
        && s.font.len() <= 256
        && valid_color(&s.color)
        && valid_color(&s.active_color)
        && bounded(s.position.x, 0.0, 1.0)
        && bounded(s.position.y, 0.0, 1.0)
        && bounded(s.max_width, 0.001, 1.0)
        && s.outline.as_ref().is_none_or(|o| bounded(o.width, 0.0, 64.0) && valid_color(&o.color))
        && s.shadow
            .as_ref()
            .is_none_or(|v| bounded(v.blur, 0.0, 64.0) && bounded(v.offset_y, -256.0, 256.0) && bounded(v.opacity, 0.0, 1.0) && valid_color(&v.color))
        && s.r#box
            .as_ref()
            .is_none_or(|b| bounded(b.padding, 0.0, 256.0) && bounded(b.radius, 0.0, 256.0) && bounded(b.opacity, 0.0, 1.0) && valid_color(&b.color))
}
fn track(params: Value, duration: Tick) -> Result<CaptionTrack> {
    let mut p: Set = serde_json::from_value(params).map_err(|_| invalid())?;
    if !valid_style(&p.style) || p.cues.len() > 10000 {
        return Err(invalid());
    }
    // Only the advertised font families are selectable; unknown names fall back to Inter.
    if !fonts().iter().any(|name| name.eq_ignore_ascii_case(&p.style.font)) {
        p.style.font = "Inter".into();
    }
    let mut track = CaptionTrack::new(TrackId(1), "Legendas".into(), CaptionFormat::Subtitle);
    let mut cues = Vec::with_capacity(p.cues.len());
    let mut previous = Tick::ZERO;
    for (i, c) in p.cues.into_iter().enumerate() {
        if !c.start.is_finite()
            || !c.end.is_finite()
            || c.start < 0.0
            || c.end > duration.seconds()
            || c.end <= c.start
            || c.words.is_empty()
            || c.words.len() > 256
        {
            return Err(invalid());
        }
        let (start, end) = (Tick::from_seconds_f64(c.start), Tick::from_seconds_f64(c.end));
        if start < previous || end <= start {
            return Err(invalid());
        }
        previous = end;
        let mut words = Vec::with_capacity(c.words.len());
        let mut last = start;
        for w in c.words {
            if w.text.trim().is_empty()
                || w.text.len() > 1024
                || w.text.chars().any(char::is_control)
                || !w.start.is_finite()
                || !w.end.is_finite()
                || w.start < c.start
                || w.end > c.end
                || w.end <= w.start
            {
                return Err(invalid());
            }
            let (ws, we) = (Tick::from_seconds_f64(w.start), Tick::from_seconds_f64(w.end));
            if ws < last || we <= ws {
                return Err(invalid());
            }
            last = we;
            words.push(CaptionWord { text: w.text, start: ws, end: we });
        }
        let text = words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
        if text.len() > 8192 {
            return Err(invalid());
        }
        track.captions.push(Caption { id: ClipId(i as u64 + 1), start, duration: end - start, text, speaker: None, cue_id: None, settings: String::new() });
        cues.push(AnimatedCue { start, end, words });
    }
    track.animation = Some(AnimatedCaptions { cues, style: p.style });
    Ok(track)
}
pub fn set(session: &mut Session, params: Value) -> Result<()> {
    let sequence = session.state.active_sequence.ok_or_else(no_session)?;
    let duration = session.active_sequence().ok_or_else(no_session)?.duration();
    let mut track = track(params, duration)?;
    let project = Arc::make_mut(&mut session.project);
    track.id = TrackId(project.alloc_id());
    for caption in &mut track.captions {
        caption.id = ClipId(project.alloc_id());
    }
    let seq = project.sequence_mut(sequence).ok_or_else(no_session)?;
    seq.caption_tracks = vec![track];
    Ok(())
}
pub fn clear(session: &mut Session) -> Result<()> {
    let sequence = session.state.active_sequence.ok_or_else(no_session)?;
    Arc::make_mut(&mut session.project).sequence_mut(sequence).ok_or_else(no_session)?.caption_tracks.clear();
    Ok(())
}
pub fn export_srt(session: &Session, params: Output, cache: &Path) -> Result<Value> {
    let seq = session.active_sequence().ok_or_else(no_session)?;
    let path = Path::new(&params.path);
    if params.path.contains("://")
        || params.path.starts_with(r"\\")
        || params.path.starts_with("//")
        || path.extension().is_none_or(|ext| !ext.eq_ignore_ascii_case("srt"))
    {
        return Err(invalid());
    }
    let doc = seq.caption_tracks.first().map(|t| filmcraft_captions::document_from_track(t, Tick::ZERO)).unwrap_or_default();
    let bytes = filmcraft_captions::write(&doc, filmcraft_captions::Format::Srt, filmcraft_captions::WriteOptions::default());
    let io_error = || error("captions_io", "Não foi possível salvar o arquivo de legendas. Confira o destino.");
    let mut temp = tempfile::NamedTempFile::new_in(cache).map_err(|_| io_error())?;
    temp.write_all(&bytes).map_err(|_| io_error())?;
    temp.as_file().sync_all().map_err(|_| io_error())?;
    crate::exports::publish(temp, path, &filmcraft_export::Progress::default())
        .map_err(|_| error("captions_io", "Não foi possível salvar sem sobrescrever o destino. Escolha outro nome."))?;
    Ok(json!({"path":params.path}))
}

#[cfg(test)]
#[path = "../tests/caption_cases/mod.rs"]
mod cases;
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_complete_replacement_and_font_fallback() {
        let duration = Tick::from_seconds_f64(60.0);
        let mut p = cases::captions("pop", 60);
        p["style"]["font"] = json!("Fonte inexistente 123");
        let t = track(p, duration).unwrap();
        assert_eq!(t.animation.unwrap().style.font, "Inter");
        for (key, value) in [("mode", json!("outro")), ("size", json!(-1)), ("weight", json!(901)), ("maxWidth", json!(0)), ("color", json!("#FFF"))] {
            let mut p = cases::captions("phrase", 60);
            p["style"][key] = value;
            assert_eq!(track(p, duration).unwrap_err().code, "captions_invalid");
        }
        for (key, value) in [("start", json!(-1)), ("end", json!(61)), ("words", json!([]))] {
            let mut p = cases::captions("phrase", 60);
            p["cues"][0][key] = value;
            assert_eq!(track(p, duration).unwrap_err().code, "captions_invalid");
        }
        let mut p = cases::captions("phrase", 60);
        p["cues"][1]["start"] = json!(2.5);
        assert_eq!(track(p, duration).unwrap_err().code, "captions_invalid");
        let mut p = cases::captions("phrase", 60);
        p["cues"][0]["words"][0]["text"] = json!("\n");
        assert_eq!(track(p, duration).unwrap_err().code, "captions_invalid");
    }
}
