//! Shared word animation rasterizer for preview and export. Legacy captions stay unchanged.
use crate::burn::{Overlay, lin};
use filmcraft_project::caption_animation::{AnimatedCaptions, CaptionMode};
use filmcraft_text::{Align, Layout, Mask, ParagraphStyle, TextStyle, layout, mask, render};
use filmcraft_time::Tick;

/// Parse an opaque protocol colour. Validation belongs to the caller.
pub fn color(value: &str, opacity: f32) -> Option<[u8; 4]> {
    if value.len() != 7 || !value.starts_with('#') || !value.is_ascii() {
        return None;
    }
    Some([
        u8::from_str_radix(&value[1..3], 16).ok()?,
        u8::from_str_radix(&value[3..5], 16).ok()?,
        u8::from_str_radix(&value[5..7], 16).ok()?,
        (opacity.clamp(0.0, 1.0) * 255.0).round() as u8,
    ])
}

pub fn pop_scale(elapsed: f64) -> f32 {
    if elapsed < 0.08 {
        1.0 + 0.22 * (elapsed.max(0.0) / 0.08) as f32
    } else if elapsed < 0.16 {
        1.22 - 0.10 * ((elapsed - 0.08) / 0.08) as f32
    } else {
        1.12
    }
}

fn paint(out: &mut [f32], cover: &Mask, rgba: [u8; 4]) {
    let c = lin(rgba);
    for (p, a) in out.as_chunks_mut::<4>().0.iter_mut().zip(&cover.a) {
        for k in 0..4 {
            p[k] = c[k] * a + p[k] * (1.0 - c[3] * a);
        }
    }
}

/// Caption visible at `t`. Gaps hold the previous word only in Word mode.
pub fn overlay(data: &AnimatedCaptions, t: Tick, w: usize, h: usize) -> Option<Overlay> {
    if w == 0 || h == 0 {
        return None;
    }
    let idx = data.cues.partition_point(|c| c.start <= t).checked_sub(1)?;
    let cue = &data.cues[idx];
    if t >= cue.end {
        return None;
    }
    let s = &data.style;
    let current = cue.words.partition_point(|word| word.start <= t).checked_sub(1);
    let active = current.filter(|&i| t < cue.words[i].end);
    let indices: Vec<usize> = if s.mode == CaptionMode::Word { vec![current?] } else { (0..cue.words.len()).collect() };
    let mut text = String::new();
    let mut ranges = Vec::new();
    for &i in &indices {
        if !text.is_empty() {
            text.push(' ');
        }
        let start = text.len();
        if s.uppercase {
            text.push_str(&cue.words[i].text.to_uppercase());
        } else {
            text.push_str(&cue.words[i].text);
        }
        ranges.push(start..text.len());
    }
    let scale = h as f32 / 1080.0;
    let weight = ["Thin", "ExtraLight", "Light", "Regular", "Medium", "SemiBold", "Bold", "ExtraBold", "Black"];
    let font_style = filmcraft_text::fonts::all_faces()
        .into_iter()
        .filter(|f| f.info.family.eq_ignore_ascii_case(&s.font))
        .min_by_key(|f| (f.info.italic, f.info.weight.abs_diff(s.weight)))
        .map_or_else(|| weight[((s.weight.clamp(100, 900) + 50) / 100 - 1) as usize].to_string(), |f| f.info.style.clone());
    let ts = TextStyle { family: s.font.clone(), style: font_style, size: s.size * scale, ..Default::default() };
    let outline = s.outline.as_ref().map_or(0.0, |o| o.width * scale);
    let padding = s.r#box.as_ref().map_or(0.0, |b| b.padding * scale);
    let shadow = s.shadow.as_ref().map_or(0.0, |v| (v.blur * 3.0 + v.offset_y.abs()) * scale);
    let extra = (outline + padding + shadow + ts.size * 0.15 + 2.0).ceil();
    let max_w = (w as f32 * s.max_width - 2.0 * extra).max(1.0);
    let paragraph = ParagraphStyle { width: Some(max_w / 1.22), align: Align::Center, ..Default::default() };
    let l = layout(&text, &ts, &paragraph);
    let width = l.lines.iter().map(|line| line.width).fold(0.0f32, f32::max);
    let height = l.bounds[3] - l.bounds[1];
    let fit = (max_w / (width * 1.22).max(1.0)).min((h as f32 - 2.0 * extra).max(1.0) / (height * 1.22).max(1.0)).min(1.0);
    let ow = ((width * 1.22 * fit + 2.0 * extra).ceil() as usize).min(w);
    let oh = ((height * 1.22 * fit + 2.0 * extra).ceil() as usize).min(h);
    if ow == 0 || oh == 0 || ow.checked_mul(oh)? > 16 << 20 {
        return None;
    }
    let x = (s.position.x * w as f32 - ow as f32 / 2.0).round().clamp(0.0, (w - ow) as f32) as i32;
    let y = (s.position.y * h as f32 - oh as f32 / 2.0).round().clamp(0.0, (h - oh) as f32) as i32;
    let tx = ow as f32 / 2.0 - max_w / 1.22 * fit / 2.0;
    let ty = oh as f32 / 2.0 - (l.bounds[1] + height / 2.0) * fit;
    let empty = || Mask { w: ow, h: oh, a: vec![0.0; ow * oh] };
    let mut normal = empty();
    let mut highlighted = empty();
    for (n, &i) in indices.iter().enumerate() {
        let highlight = match s.mode {
            CaptionMode::Phrase => false,
            CaptionMode::Word => true,
            CaptionMode::Karaoke => cue.words[i].start <= t,
            CaptionMode::Pop => active == Some(i),
        };
        let mut part = Layout { glyphs: l.glyphs.iter().filter(|g| ranges[n].contains(&g.cluster)).copied().collect(), ..Default::default() };
        let factor = if s.mode == CaptionMode::Pop && active == Some(i) { pop_scale((t - cue.words[i].start).seconds()) } else { 1.0 };
        let (left, base, _) = l.caret(ranges[n].start);
        let (right, _, _) = l.caret(ranges[n].end);
        let cx = (left + right) / 2.0;
        for glyph in &mut part.glyphs {
            glyph.x = cx + (glyph.x - cx) * factor;
            glyph.y = base + (glyph.y - base) * factor;
            glyph.size *= factor;
        }
        render::draw(&part, &[fit, 0.0, 0.0, fit, tx, ty], if highlight { &mut highlighted } else { &mut normal }, (0, 0));
    }
    let mut all = empty();
    for ((a, b), c) in all.a.iter_mut().zip(&normal.a).zip(&highlighted.a) {
        *a = (*b + *c).min(1.0);
    }
    let mut out = vec![0.0; ow * oh * 4];
    if let Some(b) = &s.r#box {
        let mut rect = empty();
        let half_w = width * fit / 2.0 + padding;
        let half_h = height * fit / 2.0 + padding;
        let r = (b.radius * scale).min(half_w).min(half_h);
        for yy in 0..oh {
            for xx in 0..ow {
                let dx = (xx as f32 + 0.5 - ow as f32 / 2.0).abs() - (half_w - r);
                let dy = (yy as f32 + 0.5 - oh as f32 / 2.0).abs() - (half_h - r);
                let distance = dx.max(0.0).hypot(dy.max(0.0)) + dx.max(dy).min(0.0) - r;
                rect.a[yy * ow + xx] = (0.5 - distance).clamp(0.0, 1.0);
            }
        }
        paint(&mut out, &rect, color(&b.color, b.opacity)?);
    }
    if let Some(v) = &s.shadow {
        let blurred = mask::blur(&all, v.blur * scale);
        let shifted = mask::offset(&blurred, 0, (v.offset_y * scale).round() as i32);
        paint(&mut out, &shifted, color(&v.color, v.opacity)?);
    }
    if let Some(v) = &s.outline
        && outline > 0.0
    {
        paint(&mut out, &mask::stroke(&all, outline, filmcraft_text::StrokeKind::Outer), color(&v.color, 1.0)?);
    }
    paint(&mut out, &normal, color(&s.color, 1.0)?);
    paint(&mut out, &highlighted, color(&s.active_color, 1.0)?);
    Some(Overlay { x, y, w: ow, h: oh, px: out })
}

#[cfg(test)]
mod tests {
    use super::*;
    use filmcraft_project::caption_animation::{AnimatedCaptionStyle, AnimatedCue, CaptionPosition, CaptionWord};
    fn sample(mode: CaptionMode) -> AnimatedCaptions {
        AnimatedCaptions {
            cues: vec![AnimatedCue {
                start: Tick::ZERO,
                end: Tick::from_seconds_f64(3.0),
                words: vec![
                    CaptionWord { text: "Olá".into(), start: Tick::ZERO, end: Tick::from_seconds_f64(0.8) },
                    CaptionWord { text: "mundo".into(), start: Tick::from_seconds_f64(1.0), end: Tick::from_seconds_f64(2.0) },
                ],
            }],
            style: AnimatedCaptionStyle {
                mode,
                font: "Inter".into(),
                weight: 700,
                size: 54.0,
                color: "#FFFFFF".into(),
                active_color: "#FF0000".into(),
                outline: None,
                shadow: None,
                r#box: None,
                uppercase: false,
                position: CaptionPosition { x: 0.5, y: 0.8 },
                max_width: 0.8,
            },
        }
    }
    fn at(data: &AnimatedCaptions, t: f64) -> Overlay {
        overlay(data, Tick::from_seconds_f64(t), 1920, 1080).unwrap()
    }
    fn red(o: &Overlay) -> usize {
        o.px.as_chunks::<4>().0.iter().filter(|p| p[0] > 0.2 && p[1] < 0.01).count()
    }
    #[test]
    fn modes_boundaries_gaps_and_pop_curve() {
        let mut p = sample(CaptionMode::Phrase);
        assert_eq!(at(&p, 0.0), at(&p, 2.5));
        assert_eq!(red(&at(&p, 0.1)), 0);
        assert!(overlay(&p, Tick::from_seconds_f64(3.0), 1920, 1080).is_none());
        p.style.mode = CaptionMode::Word;
        assert_eq!(at(&p, 0.1), at(&p, 0.9));
        assert_ne!(at(&p, 0.9), at(&p, 1.0));
        assert!(red(&at(&p, 0.1)) > 100);
        p.style.mode = CaptionMode::Karaoke;
        assert_eq!(at(&p, 0.1), at(&p, 0.9));
        assert!(red(&at(&p, 1.0)) > red(&at(&p, 0.9)));
        p.style.mode = CaptionMode::Pop;
        assert_ne!(at(&p, 0.0), at(&p, 0.08));
        assert_eq!(red(&at(&p, 0.9)), 0);
        for (t, expected) in [(0.0, 1.0), (0.08, 1.22), (0.16, 1.12), (0.6, 1.12)] {
            assert!((pop_scale(t) - expected).abs() < 0.0001);
        }
    }
    #[test]
    fn wrapping_edges_and_unbroken_words_fit() {
        let mut p = sample(CaptionMode::Pop);
        p.cues[0].words[0].text = "W".repeat(300);
        p.style.max_width = 0.3;
        for (w, h) in [(1920, 1080), (1080, 1920), (3840, 2160)] {
            for position in [0.0, 0.5, 1.0] {
                p.style.position = CaptionPosition { x: position, y: position };
                let o = overlay(&p, Tick::from_seconds_f64(0.08), w, h).unwrap();
                assert!(o.x >= 0 && o.y >= 0 && o.x as usize + o.w <= w && o.y as usize + o.h <= h);
                assert!(o.w as f32 <= w as f32 * 0.3 + 2.0);
                assert!(o.px.iter().all(|v| v.is_finite()));
            }
        }
    }
}
