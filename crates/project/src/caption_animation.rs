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
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnimatedCaptions {
    pub cues: Vec<AnimatedCue>,
    pub style: AnimatedCaptionStyle,
}
