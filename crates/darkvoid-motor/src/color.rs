//! Cor, looks e LUTs do contrato v3, usando os ajustes existentes do engine.
use crate::{
    catalog,
    protocol::{Result, error},
};
use filmcraft_engine::Session;
use filmcraft_project::{EffectInstance, ParamValue, ProjectLut, find_effect};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::HashMap, io::Read, sync::Arc};

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Color {
    pub enabled: bool,
    pub params: serde_json::Map<String, Value>,
}
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Look {
    pub id: String,
    #[serde(default = "full")]
    pub intensity: f64,
}
fn full() -> f64 {
    100.0
}

// ID público, nome próprio em português, pasta e nome/índice do engine.
pub const LOOKS: &[(&str, &str, &str, &str)] = &[
    ("look.teal-orange", "Azul-petróleo e laranja", "Criativos", "1"),
    ("look.warm-film", "Filme quente", "Criativos", "2"),
    ("look.cool-blue", "Azul frio", "Criativos", "3"),
    ("look.silver-retention", "Prata retida", "Criativos", "4"),
    ("look.faded-matte", "Mate desbotado", "Criativos", "5"),
    ("look.monochrome", "Monocromático", "Criativos", "6"),
    ("look.golden-hour", "Hora dourada", "Criativos", "7"),
    ("look.night", "Noite", "Criativos", "8"),
    ("look.preset.teal-shadows-warm-skin", "Sombras azul-petróleo, pele quente", "Cinema", "Teal Shadows, Warm Skin"),
    ("look.preset.night-exterior", "Exterior noturno", "Cinema", "Night Exterior"),
    ("look.preset.golden-dusk", "Entardecer dourado", "Cinema", "Golden Dusk"),
    ("look.preset.steel-blue", "Azul aço", "Cinema", "Steel Blue"),
    ("look.preset.hard-sun", "Sol intenso", "Cinema", "Hard Sun"),
    ("look.preset.faded-print", "Cópia desbotada", "Filme", "Faded Print"),
    ("look.preset.silver-print", "Cópia com prata retida", "Filme", "Silver Retention"),
    ("look.preset.cross-process", "Revelação cruzada", "Filme", "Cross Processed"),
    ("look.preset.warm-negative", "Negativo quente", "Filme", "Warm Negative"),
    ("look.preset.neutral-mono", "Monocromático neutro", "Monocromáticos", "Neutral Mono"),
    ("look.preset.hard-mono", "Monocromático intenso", "Monocromáticos", "Hard Mono"),
    ("look.preset.sepia", "Sépia", "Monocromáticos", "Sepia Tone"),
    ("look.preset.cold-mono", "Monocromático frio", "Monocromáticos", "Cold Mono"),
    ("look.preset.lift-shadows", "Sombras abertas", "Técnicos", "Lift Shadows"),
    ("look.preset.protect-highlights", "Realces preservados", "Técnicos", "Protect Highlights"),
    ("look.preset.half-saturation", "Meia saturação", "Técnicos", "Half Saturation"),
    ("look.preset.boost-vibrance", "Vivacidade reforçada", "Técnicos", "Boost Vibrance"),
    ("look.preset.legal-range", "Faixa de sinal reduzida", "Técnicos", "Legal Range Squeeze"),
];

pub fn exposed(id: &str) -> bool {
    !["grade_mix", "look", "look_lut", "look_intensity", "hdr_white", "hdr_specular", "curves_hdr_range"].contains(&id)
}
pub fn items() -> Vec<Value> {
    let def = find_effect("lumetri").expect("cor do engine");
    let params: Vec<_> = def
        .params
        .iter()
        .filter(|p| exposed(p.id))
        .map(|p| {
            let mut spec = catalog::param_spec(p);
            spec["group"] = json!(match p.group.unwrap_or("") {
                "Basic Correction" => "Básico",
                "Creative" => "Criativo",
                "Curves" => "Curvas",
                "Color Wheels & Match" => "Rodas de cor",
                "HSL Secondary" => "HSL",
                "Vignette" => "Vinheta",
                _ => unreachable!(),
            });
            if p.id == "input_lut" {
                spec["id"] = json!("lut");
            }
            spec
        })
        .collect();
    let mut items = vec![json!({"id":"color.grade","name":"Cor","folder":["Cor"],"kind":"color","phase":"3a","params":params,"processing":"mixed"})];
    items.extend(LOOKS.iter().map(|(id, name, folder, _)| {
        json!({"id":id,"name":name,"folder":["Looks",folder],"kind":"look","phase":"3a",
        "processing":"mixed","params":[{"id":"intensity","name":"Intensidade","type":"float","min":0,"max":100,"default":100,"animatable":false,"unit":"%"}]})
    }));
    items
}
pub fn label(id: &str) -> &str {
    match id {
        "basic_on" => "Básico",
        "input_lut" => "LUT",
        "temperature" => "Temperatura",
        "tint" => "Matiz",
        "exposure" => "Exposição",
        "contrast" => "Contraste",
        "highlights" => "Realces",
        "shadows" => "Sombras",
        "whites" => "Brancos",
        "blacks" => "Pretos",
        "saturation" => "Saturação",
        "creative_on" => "Criativo",
        "faded_film" => "Filme desbotado",
        "sharpen" => "Nitidez",
        "vibrance" => "Vibração",
        "creative_sat" => "Saturação criativa",
        "shadow_tint" => "Cor das sombras",
        "highlight_tint" => "Cor dos realces",
        "curves_on" => "Curvas",
        "curve_luma" => "Curva de brilho",
        "curve_red" => "Curva vermelha",
        "curve_green" => "Curva verde",
        "curve_blue" => "Curva azul",
        "hue_vs_sat" => "Matiz por saturação",
        "hue_vs_hue" => "Matiz por matiz",
        "hue_vs_luma" => "Matiz por brilho",
        "luma_vs_sat" => "Brilho por saturação",
        "sat_vs_sat" => "Saturação por saturação",
        "wheels_on" => "Rodas de cor",
        "wheel_shadows" => "Roda das sombras",
        "wheel_midtones" => "Roda dos meios-tons",
        "wheel_highlights" => "Roda dos realces",
        "wheel_shadows_l" => "Brilho das sombras",
        "wheel_midtones_l" => "Brilho dos meios-tons",
        "wheel_highlights_l" => "Brilho dos realces",
        "hsl_on" => "Correção secundária HSL",
        "hsl_hue" => "Centro do matiz",
        "hsl_hue_range" => "Faixa de matiz",
        "hsl_sat_min" => "Saturação mínima",
        "hsl_luma_min" => "Brilho mínimo",
        "hsl_luma_max" => "Brilho máximo",
        "hsl_soft" => "Suavização",
        "hsl_show_mask" => "Mostrar seleção",
        "hsl_denoise" => "Redução de ruído",
        "hsl_blur" => "Desfoque da seleção",
        "hsl_temp" => "Temperatura da seleção",
        "hsl_tint" => "Matiz da seleção",
        "hsl_sat" => "Saturação da seleção",
        "hsl_hue_shift" => "Deslocamento do matiz",
        "vignette_on" => "Vinheta",
        "vignette_amount" => "Quantidade da vinheta",
        "vignette_midpoint" => "Ponto médio",
        "vignette_roundness" => "Arredondamento",
        "vignette_feather" => "Suavização da vinheta",
        _ => panic!("parâmetro sem tradução: {id}"),
    }
}

#[derive(Default)]
pub struct LutCache {
    entries: HashMap<Arc<str>, ProjectLut>,
    next: u64,
    bytes: usize,
}
impl LutCache {
    fn load(&mut self, path: &str, clip: &str) -> Result<ProjectLut> {
        let invalid = || error("lut_invalid", format!("Clipe '{clip}': color.params.lut deve ser .cube válido, até 16 MB e de 2 a 65 pontos."));
        if path.contains("://") || path.starts_with(r"\\") || path.starts_with("//") {
            return Err(invalid());
        }
        if !std::path::Path::new(path).extension().is_some_and(|e| e.eq_ignore_ascii_case("cube")) {
            return Err(invalid());
        }
        let file = std::fs::File::open(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                error("media_not_found", format!("Clipe '{clip}': LUT não encontrada: '{path}'."))
            } else {
                invalid()
            }
        })?;
        const LIMIT: usize = 16 * 1024 * 1024;
        if file.metadata().map_err(|_| invalid())?.len() > LIMIT as u64 {
            return Err(invalid());
        }
        let mut text = String::new();
        file.take(LIMIT as u64 + 1).read_to_string(&mut text).map_err(|_| invalid())?;
        if text.len() > LIMIT {
            return Err(invalid());
        }
        if let Some(entry) = self.entries.get(text.as_str()) {
            return Ok(entry.clone());
        }
        let mut sizes = std::collections::HashSet::new();
        for line in text.trim_start_matches('\u{feff}').lines() {
            let mut words = line.split('#').next().unwrap_or("").split_whitespace();
            let key = words.next().unwrap_or("");
            if key == "TITLE" {
                continue;
            }
            if std::iter::once(key).chain(words.clone()).any(|word| word.parse::<f64>().is_ok_and(|n| !n.is_finite())) {
                return Err(invalid());
            }
            if matches!(key, "LUT_3D_SIZE" | "LUT_1D_SIZE") {
                let n = words.next().and_then(|n| n.parse::<usize>().ok()).ok_or_else(invalid)?;
                if !(2..=65).contains(&n) || !sizes.insert(key) {
                    return Err(invalid());
                }
            }
        }
        if sizes.is_empty() {
            return Err(invalid());
        }
        filmcraft_color::Lut::parse_cube(&text).map_err(|_| invalid())?;
        if self.bytes + text.len() > 64 * 1024 * 1024 {
            self.entries.clear();
            self.bytes = 0;
        }
        self.next += 1;
        self.bytes += text.len();
        let text: Arc<str> = text.into();
        let entry =
            ProjectLut { id: format!("motor-lut-{}", self.next), name: "LUT importada".into(), source_path: None, format: "cube".into(), text: text.clone() };
        self.entries.insert(text, entry.clone());
        Ok(entry)
    }
    pub fn prepare(&mut self, color: Option<&Color>, look: Option<&Look>, clip: &str, paths: &mut HashMap<String, ProjectLut>) -> Result<Vec<EffectInstance>> {
        let mut effects = Vec::new();
        if let Some(color) = color {
            let def = find_effect("lumetri").expect("cor do engine");
            let mut effect = def.instance();
            effect.enabled = color.enabled;
            for (id, value) in &color.params {
                let internal = if id == "lut" { "input_lut" } else { id.as_str() };
                let p = def
                    .param(internal)
                    .filter(|p| exposed(p.id) && p.id != "input_lut" || id == "lut")
                    .ok_or_else(|| catalog::invalid(clip, &format!("color.params.{id}")))?;
                let parsed = if id == "lut" {
                    let path = value.as_str().ok_or_else(|| catalog::invalid(clip, "color.params.lut"))?;
                    if path.is_empty() {
                        ParamValue::Text(String::new())
                    } else {
                        if !paths.contains_key(path) {
                            paths.insert(path.into(), self.load(path, clip)?);
                        }
                        ParamValue::Text(format!("lib:{}", paths[path].id))
                    }
                } else {
                    catalog::parse_param(p, value).ok_or_else(|| catalog::invalid(clip, &format!("color.params.{id}")))?
                };
                effect.params.get_mut(internal).expect("parâmetro do engine").value = parsed;
            }
            let f = |id: &str| effect.f64_at(id, filmcraft_time::Tick::ZERO);
            if f("hsl_luma_min") > f("hsl_luma_max") {
                return Err(catalog::invalid(clip, "color.params.hsl_luma_min/hsl_luma_max"));
            }
            effects.push(effect);
        }
        if let Some(look) = look {
            if !look.intensity.is_finite() || !(0.0..=100.0).contains(&look.intensity) {
                return Err(catalog::invalid(clip, "look.intensity"));
            }
            let (_, _, _, engine) = LOOKS.iter().find(|(id, _, _, _)| *id == look.id).ok_or_else(|| catalog::invalid(clip, "look.id"))?;
            let mut effect = if let Ok(index) = engine.parse::<u32>() {
                let mut e = find_effect("lumetri").expect("cor do engine").instance();
                e.params.get_mut("look").unwrap().value = ParamValue::Choice(index);
                e
            } else {
                filmcraft_render::lumetri_presets::find(engine).expect("preset aprovado").instance()
            };
            effect.params.get_mut("grade_mix").expect("mistura do grade").value = ParamValue::Float(look.intensity);
            // Zero desliga o efeito: preserva os bytes da imagem anterior ao look.
            effect.enabled = look.intensity > 0.0;
            effects.push(effect);
        }
        Ok(effects)
    }
}

pub fn scopes(session: &Session, time: filmcraft_time::Tick, params: Value) -> Result<Value> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        kinds: Vec<String>,
    }
    let input: Input = crate::protocol::field(params)?;
    if input.kinds.iter().any(|k| !["waveform", "vectorscope"].contains(&k.as_str())) {
        return Err(crate::protocol::invalid());
    }
    let seq = session.active_sequence().ok_or_else(|| error("no_session", "Abra uma sessão antes de calcular os osciloscópios."))?;
    if input.kinds.is_empty() {
        return Ok(json!({}));
    }
    let sources = session.media.provider(session.project.clone(), session.services.clone());
    let (w, h, pixels) = filmcraft_render::render_sequence_rgba8(
        &session.project,
        session.state.active_sequence.unwrap(),
        time,
        filmcraft_render::RenderOptions { scale: (480.0 / seq.settings.width as f32).min(270.0 / seq.settings.height as f32).min(1.0), ..Default::default() },
        &sources,
    );
    let signal = filmcraft_scopes::Signal::from_rgba8(w, h, &pixels);
    let settings = filmcraft_scopes::Params::default();
    let mut result = json!({});
    for kind in input.kinds {
        let (w, h, mut pixels) = if kind == "waveform" {
            let waveform = filmcraft_scopes::waveform(&signal, filmcraft_scopes::WaveformType::Luma, &settings);
            filmcraft_scopes::paint::grids(&[(&waveform.traces[0], [0.3, 1.0, 0.45])], filmcraft_scopes::paint::waveform_reference(waveform.per_column), 1.0)
        } else {
            filmcraft_scopes::paint::vectorscope(&filmcraft_scopes::vectorscope_yuv(&signal, &settings), [0.3, 1.0, 0.45], 1.0, true)
        };
        // O pintor do engine retorna pixels premultiplicados. PNG usa alfa reto;
        // apresenta os traços sobre preto, sem multiplicar a densidade novamente.
        for pixel in pixels.as_chunks_mut::<4>().0 {
            pixel[3] = 255;
        }
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_raw(w as u32, h as u32, pixels)
            .ok_or_else(|| error("scopes_failed", "Não foi possível calcular os osciloscópios."))?
            .write_to(&mut png, image::ImageFormat::Png)
            .map_err(|_| error("scopes_failed", "Não foi possível codificar os osciloscópios."))?;
        use base64::Engine;
        result[&kind] = json!(base64::engine::general_purpose::STANDARD.encode(png.into_inner()));
    }
    Ok(result)
}
