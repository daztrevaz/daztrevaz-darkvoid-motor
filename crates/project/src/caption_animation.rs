//! Optional word animation for caption tracks. Old tracks keep their original renderer.
use filmcraft_time::Tick;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CaptionMode {
    Phrase,
    Word,
    Karaoke,
    Pop,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnimatedCaptionStyle {
    pub mode: CaptionMode,
    pub font: String,
    pub weight: u16,
    pub size: f32,
    pub color: String,
    pub active_color: String,
    pub outline: Option<CaptionOutline>,
    pub shadow: Option<CaptionShadow>,
    pub r#box: Option<CaptionBox>,
    pub uppercase: bool,
    pub position: CaptionPosition,
    pub max_width: f32,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CaptionPosition {
    pub x: f32,
    pub y: f32,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CaptionOutline {
    pub width: f32,
    pub color: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptionShadow {
    pub blur: f32,
    pub offset_y: f32,
    pub color: String,
    pub opacity: f32,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CaptionBox {
    pub color: String,
    pub opacity: f32,
    pub padding: f32,
    pub radius: f32,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CaptionWord {
    pub text: String,
    pub start: Tick,
    pub end: Tick,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnimatedCue {
    pub start: Tick,
    pub end: Tick,
    pub words: Vec<CaptionWord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#override: Option<CaptionOverride>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CaptionOverride {
    pub position: Option<CaptionPosition>,
    pub size: Option<f32>,
    pub max_width: Option<f32>,
}
impl CaptionOverride {
    pub fn apply(&self, style: &AnimatedCaptionStyle) -> AnimatedCaptionStyle {
        let mut resolved = style.clone();
        if let Some(position) = &self.position {
            resolved.position = position.clone();
        }
        if let Some(size) = self.size {
            resolved.size = size;
        }
        if let Some(max_width) = self.max_width {
            resolved.max_width = max_width;
        }
        resolved
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnimatedCaptions {
    pub cues: Vec<AnimatedCue>,
    pub style: AnimatedCaptionStyle,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cue_override_is_optional_and_round_trips() {
        let mut cue = AnimatedCue { start: Tick::ZERO, end: Tick::from_seconds_f64(1.0), words: vec![], r#override: None };
        let legacy = serde_json::to_value(&cue).unwrap();
        assert!(legacy.get("override").is_none());
        assert_eq!(serde_json::from_value::<AnimatedCue>(legacy).unwrap(), cue);
        cue.r#override = Some(CaptionOverride { position: Some(CaptionPosition { x: 0.2, y: 0.3 }), size: Some(90.0), max_width: Some(0.4) });
        let value = serde_json::to_value(&cue).unwrap();
        assert_eq!(value["override"]["maxWidth"], 0.4f32);
        assert_eq!(serde_json::from_value::<AnimatedCue>(value).unwrap(), cue);
    }
}
