//! Catálogo público v3 e conversão validada dos parâmetros do engine.
use crate::protocol::{Result, error};
use filmcraft_project::{EffectInstance, ParamDef, ParamKind, ParamValue, find_effect};
use serde_json::{Value, json};

pub const TRANSITIONS: &[(&str, &str, &str)] = &[
    ("cross_dissolve", "Dissolução cruzada", "Dissoluções"),
    ("dip_to_black", "Passagem pelo preto", "Dissoluções"),
    ("dip_to_white", "Passagem pelo branco", "Dissoluções"),
    ("dip_to_color", "Passagem por cor", "Dissoluções"),
    ("blur_dissolve", "Dissolução com desfoque", "Dissoluções"),
    ("additive_dissolve", "Dissolução aditiva", "Dissoluções"),
    ("film_dissolve", "Dissolução de filme", "Dissoluções"),
    ("luma_fade", "Dissolução por brilho", "Dissoluções"),
    ("mosaic_transition", "Mosaico", "Dissoluções"),
    ("flip_motion", "Virar", "Animação"),
    ("fold_motion", "Dobrar", "Animação"),
    ("pop_motion", "Saltar", "Animação"),
    ("spin_motion", "Girar", "Animação"),
    ("spring_motion", "Mola", "Animação"),
    ("travel_motion", "Viagem", "Animação"),
    ("cross_zoom", "Zoom cruzado", "Luzes e desfoques"),
    ("zoom_blur", "Desfoque de zoom", "Luzes e desfoques"),
    ("radial_blur", "Desfoque radial", "Luzes e desfoques"),
    ("directional_blur_transition", "Desfoque direcional", "Luzes e desfoques"),
    ("lens_blur", "Desfoque de lente", "Luzes e desfoques"),
    ("flash", "Clarão", "Luzes e desfoques"),
    ("flare", "Reflexo de lente", "Luzes e desfoques"),
    ("glow", "Brilho", "Luzes e desfoques"),
    ("light_leak", "Vazamento de luz", "Luzes e desfoques"),
    ("light_sweep", "Varredura de luz", "Luzes e desfoques"),
    ("chroma_leak", "Vazamento de cor", "Luzes e desfoques"),
    ("glitch", "Falha digital", "Distorções"),
    ("vhs_damage", "Fita VHS", "Distorções"),
    ("tv_power", "TV desligando", "Distorções"),
    ("earthquake", "Terremoto", "Distorções"),
    ("flicker", "Tremulação", "Distorções"),
    ("glass", "Vidro", "Distorções"),
    ("kaleidoscope", "Caleidoscópio", "Distorções"),
    ("liquid_distortion", "Distorção líquida", "Distorções"),
    ("push", "Empurrar", "Deslocamentos"),
    ("slide", "Deslizar", "Deslocamentos"),
    ("split", "Dividir", "Deslocamentos"),
    ("whip", "Chicote", "Deslocamentos"),
    ("roll", "Rolar", "Deslocamentos"),
    ("stretch", "Esticar", "Deslocamentos"),
    ("spin_3d", "Giro 3D", "Transformações"),
    ("page_peel", "Virar página", "Transformações"),
    ("louver", "Persiana", "Transformações"),
    ("slice", "Fatiar", "Transformações"),
    ("wave", "Onda", "Transformações"),
    ("linear_wipe", "Varredura linear", "Varreduras"),
    ("soft_wipe", "Varredura suave", "Varreduras"),
    ("radial_wipe", "Varredura radial", "Varreduras"),
    ("clock_wipe", "Relógio", "Varreduras"),
    ("star_wipe", "Estrela", "Varreduras"),
];

pub fn public_transition(id: &str) -> String {
    format!(
        "transition.{}",
        match id {
            "mosaic_transition" => "mosaic",
            "directional_blur_transition" => "directional_blur",
            _ => id,
        }
    )
}

pub fn get() -> Value {
    let mut items: Vec<_> = TRANSITIONS
        .iter()
        .map(|(id, name, folder)| {
            let def = find_effect(id).expect("transição do engine");
            json!({"id":public_transition(id),"name":name,"folder":["Transições",folder],"kind":"transition","phase":"3a",
            "params":def.params.iter().map(param_spec).collect::<Vec<_>>(),"processing":"mixed","defaultDuration":1.0})
        })
        .collect();
    items.extend(crate::color::items());
    items.extend(crate::effects::items());
    json!({"version":3,"revision":"m3.3-1","items":items})
}

fn label(id: &str) -> &str {
    match id {
        "direction" => "Direção",
        "perspective" => "Perspectiva",
        "motion_blur" => "Desfoque de movimento",
        "overshoot" => "Excesso",
        "rotation" => "Rotação",
        "spin" => "Sentido do giro",
        "bounce" => "Ressalto",
        "zoom" => "Zoom",
        "blur" => "Desfoque",
        "color" => "Cor",
        "hold" => "Permanência",
        "source" => "Origem do brilho",
        "softness" => "Suavidade",
        "invert" => "Inverter",
        "block_size" => "Tamanho máximo dos blocos",
        "dissolve" => "Dissolver blocos",
        "center" => "Centro",
        "strength" => "Força",
        "angle" => "Ângulo",
        "intensity" => "Intensidade",
        "radius" => "Raio",
        "amount" => "Quantidade",
        "seed" => "Semente aleatória",
        "width" => "Largura",
        "shake" => "Tremor",
        "flickers" => "Tremulações",
        "cell_size" => "Tamanho dos fragmentos",
        "refraction" => "Refração",
        "segments" => "Segmentos",
        "scale" => "Escala",
        "glow_color" => "Cor do brilho",
        "line" => "Espessura da linha",
        "orientation" => "Orientação",
        "slats" => "Lâminas",
        "corner" => "Canto",
        "shadow" => "Sombra",
        "slices" => "Fatias",
        "amplitude" => "Amplitude",
        "wavelength" => "Comprimento da onda",
        "start" => "Ângulo inicial",
        "feather" => "Suavização da borda",
        "border_width" => "Largura da borda",
        "border_color" => "Cor da borda",
        "antialias" => "Qualidade da suavização",
        "points" => "Pontas",
        "inner" => "Raio interno",
        _ => crate::color::label(id),
    }
}
fn choice_name(name: &str) -> &str {
    match name {
        "From North" => "De cima",
        "From East" => "Da direita",
        "From South" => "De baixo",
        "From West" => "Da esquerda",
        "Clockwise" => "Horário",
        "Counter-clockwise" => "Anti-horário",
        "Outgoing" => "Clipe anterior",
        "Incoming" => "Clipe seguinte",
        "Horizontal" => "Horizontal",
        "Vertical" => "Vertical",
        "Top Left" => "Superior esquerdo",
        "Top Right" => "Superior direito",
        "Bottom Right" => "Inferior direito",
        "Bottom Left" => "Inferior esquerdo",
        "Off" => "Desligada",
        "Low" => "Baixa",
        "Medium" => "Média",
        "High" => "Alta",
        "Color/Gray" => "Cor e cinza",
        "Color/Black" => "Cor e preto",
        "White/Black" => "Branco e preto",
        _ => panic!("opção sem tradução: {name}"),
    }
}
pub fn param_spec(p: &ParamDef) -> Value {
    named_spec(p, label(p.id), |name| choice_name(name).to_owned())
}
pub fn named_spec(p: &ParamDef, name: &str, choice: fn(&str) -> String) -> Value {
    let (kind, min, max, unit) = match p.kind {
        ParamKind::Float { min, max, unit, .. } => ("float", Some(min), Some(max), unit),
        ParamKind::Angle => ("float", Some(-36000.0), Some(36000.0), "°"),
        ParamKind::Bool => ("bool", None, None, ""),
        ParamKind::Choice(_) => ("choice", None, None, ""),
        ParamKind::Point => ("point", None, None, "px"),
        ParamKind::Color => ("color", Some(0.0), Some(1.0), ""),
        ParamKind::Curve { .. } => ("curve", Some(0.0), Some(1.0), ""),
        ParamKind::Wheel => ("wheel", Some(-1.0), Some(1.0), ""),
        ParamKind::Text => ("lut", None, None, ""),
        _ => unreachable!("tipo fora do catálogo M3.1"),
    };
    let default = match &p.default {
        ParamValue::Float(n) => json!(n),
        ParamValue::Bool(b) => json!(b),
        ParamValue::Choice(n) => json!(n.to_string()),
        ParamValue::Color(c) => json!(c),
        ParamValue::Vec2(v) if v.x.is_finite() && v.y.is_finite() => json!([v.x, v.y]),
        ParamValue::Vec2(_) => Value::Null,
        ParamValue::Curve(points) => json!(points),
        ParamValue::Text(text) => json!(text),
        _ => unreachable!(),
    };
    let mut spec = json!({"id":p.id,"name":name,"type":kind,"min":min,"max":max,"default":default,"animatable":false});
    if !unit.is_empty() {
        spec["unit"] = json!(unit);
    }
    if kind == "point" && spec["default"].is_null() {
        spec["defaultFrom"] = json!("frameCenter");
    }
    if let ParamKind::Choice(options) = p.kind {
        spec["choices"] = json!(options.iter().enumerate().map(|(i, name)| json!({"id":i.to_string(),"name":choice(name)})).collect::<Vec<_>>());
    }
    spec
}

pub fn transition(id: &str, params: &serde_json::Map<String, Value>, clip: &str) -> Result<EffectInstance> {
    let internal =
        TRANSITIONS.iter().find(|(internal, _, _)| public_transition(internal) == id).map(|v| v.0).ok_or_else(|| invalid(clip, "transitionIn.id"))?;
    let def = find_effect(internal).expect("transição do engine");
    let mut effect = def.instance();
    for (id, value) in params {
        let p = def.param(id).ok_or_else(|| invalid(clip, &format!("transitionIn.params.{id}")))?;
        let parsed = parse_param(p, value).ok_or_else(|| invalid(clip, &format!("transitionIn.params.{id}")))?;
        effect.params.get_mut(id).expect("parâmetro do engine").value = parsed;
    }
    Ok(effect)
}
pub fn invalid(clip: &str, field: &str) -> crate::protocol::Error {
    error("timeline_invalid", format!("Clipe '{clip}': campo {field} inválido ou não disponível no catálogo."))
}
pub fn parse_param(p: &ParamDef, v: &Value) -> Option<ParamValue> {
    match &p.kind {
        ParamKind::Float { min, max, .. } => v.as_f64().filter(|n| n.is_finite() && (*min..=*max).contains(n)).map(ParamValue::Float),
        ParamKind::Angle => v.as_f64().filter(|n| n.is_finite() && (-36000.0..=36000.0).contains(n)).map(ParamValue::Float),
        ParamKind::Bool => v.as_bool().map(ParamValue::Bool),
        ParamKind::Choice(options) => v.as_str().and_then(|s| (0..options.len()).find(|i| i.to_string() == s)).map(|i| ParamValue::Choice(i as u32)),
        ParamKind::Point => {
            let a = v.as_array()?;
            if a.len() != 2 {
                return None;
            }
            let (x, y) = (a[0].as_f64()?, a[1].as_f64()?);
            if ![x, y].iter().all(|n| n.is_finite() && n.abs() <= 100000.0) {
                return None;
            }
            serde_json::from_value(json!({"Vec2":{"x":x,"y":y}})).ok()
        }
        ParamKind::Color => {
            let a = v.as_array()?;
            if a.len() != 4 {
                return None;
            }
            let mut color = [0.0; 4];
            for (out, value) in color.iter_mut().zip(a) {
                *out = value.as_f64().filter(|n| n.is_finite() && (0.0..=1.0).contains(n))? as f32;
            }
            Some(ParamValue::Color(color))
        }
        ParamKind::Wheel => {
            let a = v.as_array()?;
            if a.len() != 2 {
                return None;
            }
            let (x, y) = (a[0].as_f64()?, a[1].as_f64()?);
            if ![x, y].iter().all(|n| n.is_finite() && (-1.0..=1.0).contains(n)) {
                return None;
            }
            serde_json::from_value(json!({"Vec2":{"x":x,"y":y}})).ok()
        }
        ParamKind::Curve { .. } => {
            let values = v.as_array()?;
            if values.len() == 1 || values.len() > 256 {
                return None;
            }
            let mut points = Vec::<[f32; 2]>::with_capacity(values.len());
            for value in values {
                let a = value.as_array()?;
                if a.len() != 2 {
                    return None;
                }
                let (x, y) = (a[0].as_f64()?, a[1].as_f64()?);
                if ![x, y].iter().all(|n| n.is_finite() && (0.0..=1.0).contains(n)) || points.last().is_some_and(|p| p[0] >= x as f32) {
                    return None;
                }
                points.push([x as f32, y as f32]);
            }
            Some(ParamValue::Curve(points))
        }
        _ => None,
    }
}
