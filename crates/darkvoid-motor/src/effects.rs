//! Contrato público de efeitos e movimento, reutilizando os parâmetros do engine.
use crate::{catalog, protocol::Result};
use filmcraft_project::{EffectInstance, Interpolation, Keyframe, Param, ParamDef, ParamValue, find_effect};
use filmcraft_time::Tick;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashSet};

pub const EFFECTS: &[(&str, &str, &str)] = &[
    ("gaussian_blur", "Desfoque gaussiano", "Desfoque e nitidez"),
    ("directional_blur", "Desfoque direcional", "Desfoque e nitidez"),
    ("bokeh_blur", "Desfoque bokeh", "Desfoque e nitidez"),
    ("focus_blur", "Desfoque de foco", "Desfoque e nitidez"),
    ("compound_blur", "Desfoque composto", "Desfoque e nitidez"),
    ("channel_blur", "Desfoque por canal", "Desfoque e nitidez"),
    ("sharpen", "Nitidez", "Desfoque e nitidez"),
    ("unsharp_mask", "Máscara de nitidez", "Desfoque e nitidez"),
    ("lens_distortion", "Distorção de lente", "Distorção"),
    ("spherize", "Esferizar", "Distorção"),
    ("twirl", "Redemoinho", "Distorção"),
    ("wave_warp", "Ondulação", "Distorção"),
    ("turbulent_displace", "Deslocamento turbulento", "Distorção"),
    ("magnify", "Lupa", "Distorção"),
    ("mirror", "Espelho", "Distorção"),
    ("offset", "Deslocar imagem", "Distorção"),
    ("corner_pin", "Fixar cantos", "Distorção"),
    ("camera_shake", "Tremor de câmera", "Movimento e câmera"),
    ("wiggle", "Balanço", "Movimento e câmera"),
    ("strobe", "Estroboscópio", "Movimento e câmera"),
    ("posterize_time", "Redução de quadros", "Movimento e câmera"),
    ("basic_3d", "3D básico", "Movimento e câmera"),
    ("rotate_3d", "Rotação 3D", "Movimento e câmera"),
    ("lens_flare", "Reflexo de lente", "Luzes e brilho"),
    ("light_leaks", "Vazamento de luz", "Luzes e brilho"),
    ("glint", "Cintilar", "Luzes e brilho"),
    ("edge_glow", "Brilho nas bordas", "Luzes e brilho"),
    ("wonder_glow", "Brilho mágico", "Luzes e brilho"),
    ("volumetric_rays", "Raios de luz", "Luzes e brilho"),
    ("lighting_effects", "Iluminação", "Luzes e brilho"),
    ("echo_glow", "Brilho com eco", "Luzes e brilho"),
    ("black_white", "Preto e branco", "Estilização"),
    ("invert", "Inverter cores", "Estilização"),
    ("find_edges", "Contornos", "Estilização"),
    ("color_emboss", "Relevo colorido", "Estilização"),
    ("mosaic", "Mosaico", "Estilização"),
    ("noise", "Ruído", "Estilização"),
    ("rgb_split", "Separação RGB", "Estilização"),
    ("brush_strokes", "Pinceladas", "Estilização"),
    ("roughen_edges", "Bordas ásperas", "Estilização"),
    ("stroke", "Contorno", "Estilização"),
    ("drop_shadow", "Sombra projetada", "Estilização"),
    ("long_shadow", "Sombra longa", "Estilização"),
    ("vignette", "Vinheta", "Estilização"),
    ("tint", "Tingir", "Cor"),
    ("color_pass", "Manter uma cor", "Cor"),
    ("color_replace", "Trocar cor", "Cor"),
    ("levels", "Níveis", "Cor"),
    ("channel_mix", "Mixer de canais", "Cor"),
    ("brightness_contrast", "Brilho e contraste", "Cor"),
    ("four_color_gradient", "Gradiente de 4 cores", "Cor"),
    ("gradient", "Gradiente", "Cor"),
    ("horizontal_flip", "Inverter na horizontal", "Transformação"),
    ("vertical_flip", "Inverter na vertical", "Transformação"),
    ("rounded_crop", "Recorte arredondado", "Transformação"),
    ("warp_stabilizer", "Estabilizador", "Transformação"),
    ("ultra_key", "Chroma key", "Chaves e máscaras"),
    ("color_key", "Chave de cor", "Chaves e máscaras"),
    ("luma_key", "Chave de brilho", "Chaves e máscaras"),
    ("track_matte", "Máscara por trilha", "Chaves e máscaras"),
    ("alpha_adjust", "Ajuste de transparência", "Chaves e máscaras"),
];

// Dissolve é uma transição; os outros 26 modos seguem a ordem do engine.
pub const BLENDS: &[(&str, &str)] = &[
    ("normal", "Normal"),
    ("darken", "Escurecer"),
    ("multiply", "Multiplicar"),
    ("color_burn", "Subexposição de cor"),
    ("linear_burn", "Subexposição linear"),
    ("darker_color", "Cor mais escura"),
    ("lighten", "Clarear"),
    ("screen", "Trama"),
    ("color_dodge", "Superexposição de cor"),
    ("linear_dodge", "Adicionar"),
    ("lighter_color", "Cor mais clara"),
    ("overlay", "Sobrepor"),
    ("soft_light", "Luz suave"),
    ("hard_light", "Luz intensa"),
    ("vivid_light", "Luz vívida"),
    ("linear_light", "Luz linear"),
    ("pin_light", "Luz pontual"),
    ("hard_mix", "Mistura sólida"),
    ("difference", "Diferença"),
    ("exclusion", "Exclusão"),
    ("subtract", "Subtrair"),
    ("divide", "Dividir"),
    ("hue", "Matiz"),
    ("saturation", "Saturação"),
    ("color", "Cor"),
    ("luminosity", "Luminosidade"),
];

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Animated {
    value: Value,
    #[serde(default)]
    keyframes: Vec<Point>,
}
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Point {
    time: f64,
    value: Value,
    interpolation: String,
}
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Effect {
    pub id: String,
    pub instance_id: String,
    pub enabled: bool,
    pub params: BTreeMap<String, Animated>,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Transform {
    position: Option<Animated>,
    scale: Option<Animated>,
    rotation: Option<Animated>,
    opacity: Option<Animated>,
    blend_mode: Option<String>,
    crop: Option<Crop>,
}
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Crop {
    left: Animated,
    top: Animated,
    right: Animated,
    bottom: Animated,
}

fn track_choice(p: &ParamDef) -> bool {
    matches!(&p.kind, filmcraft_project::ParamKind::Choice(options) if *options == filmcraft_project::effect::TRACK_CHOICES)
}
fn parse_value(p: &ParamDef, value: &Value) -> Option<ParamValue> {
    if track_choice(p) && !matches!(value.as_str(), Some("0" | "1" | "2")) {
        return None;
    }
    catalog::parse_param(p, value)
}
pub fn animated(p: &ParamDef, input: &Animated, clip: &str, field: &str) -> Result<Param> {
    let invalid = || catalog::invalid(clip, field);
    let mut param = Param::new(parse_value(p, &input.value).ok_or_else(invalid)?);
    let mut last = None;
    for point in &input.keyframes {
        if !p.animatable || !point.time.is_finite() || point.time < 0.0 || point.time >= Tick(i64::MAX).seconds() {
            return Err(invalid());
        }
        let time = Tick::from_seconds_f64(point.time);
        if last.is_some_and(|prev| prev >= time) {
            return Err(invalid());
        }
        last = Some(time);
        let mut key = Keyframe::new(time, parse_value(p, &point.value).ok_or_else(invalid)?);
        key.interp = match point.interpolation.as_str() {
            "linear" => Interpolation::Linear,
            "easeIn" => Interpolation::EaseIn,
            "easeOut" => Interpolation::EaseOut,
            "hold" => Interpolation::Hold,
            _ => return Err(invalid()),
        };
        param.keyframes.push(key);
    }
    Ok(param)
}

pub fn prepare(effects: &[Effect], transform: Option<&Transform>, clip: &str) -> Result<Vec<EffectInstance>> {
    let mut out = Vec::new();
    let mut ids = HashSet::new();
    for input in effects {
        if input.instance_id.trim().is_empty() || !ids.insert(&input.instance_id) {
            return Err(catalog::invalid(clip, "effects.instanceId"));
        }
        let id = input.id.strip_prefix("effect.").filter(|id| EFFECTS.iter().any(|e| e.0 == *id)).ok_or_else(|| catalog::invalid(clip, "effects.id"))?;
        let def = find_effect(id).expect("efeito aprovado");
        let mut effect = def.instance();
        effect.enabled = input.enabled;
        for (id, value) in &input.params {
            let field = format!("effects.{}.params.{id}", input.instance_id);
            let p = def.param(id).ok_or_else(|| catalog::invalid(clip, &field))?;
            effect.params.insert(id.clone(), animated(p, value, clip, &field)?);
        }
        out.push(effect);
    }
    if let Some(t) = transform {
        if let Some(crop) = &t.crop {
            let def = find_effect("crop").expect("recorte");
            let mut effect = def.instance();
            for (id, value) in [("left", &crop.left), ("top", &crop.top), ("right", &crop.right), ("bottom", &crop.bottom)] {
                effect.params.insert(id.into(), animated(def.param(id).unwrap(), value, clip, &format!("transform.crop.{id}"))?);
            }
            out.push(effect);
        }
        for (id, values) in [
            ("motion", vec![("position", t.position.as_ref()), ("scale", t.scale.as_ref()), ("rotation", t.rotation.as_ref())]),
            ("opacity", vec![("opacity", t.opacity.as_ref())]),
        ] {
            let def = find_effect(id).expect("transformação");
            let mut effect = def.instance();
            for (param, value) in values {
                if let Some(value) = value {
                    effect.params.insert(param.into(), animated(def.param(param).unwrap(), value, clip, &format!("transform.{param}"))?);
                }
            }
            if id == "opacity"
                && let Some(mode) = &t.blend_mode
            {
                let n = BLENDS.iter().position(|b| b.0 == mode).ok_or_else(|| catalog::invalid(clip, "transform.blendMode"))?;
                effect.params.insert("blend".into(), Param::new(ParamValue::Choice(if n == 0 { 0 } else { n as u32 + 1 })));
            }
            out.push(effect);
        }
    }
    Ok(out)
}

pub fn items() -> Vec<Value> {
    let mut items: Vec<_> = EFFECTS
        .iter()
        .map(|(id, name, folder)| {
            let def = find_effect(id).expect("efeito aprovado");
            json!({"id":format!("effect.{id}"),"name":name,"folder":["Efeitos",folder],"kind":"effect","phase":"3b",
            "params":def.params.iter().map(spec).collect::<Vec<_>>(),"processing":"mixed"})
        })
        .collect();
    let mut params = Vec::new();
    for (effect, id) in [("motion", "position"), ("motion", "scale"), ("motion", "rotation"), ("opacity", "opacity")] {
        let mut p = spec(find_effect(effect).unwrap().param(id).unwrap());
        if id == "position" {
            p["defaultFrom"] = json!("frameCenter");
        }
        params.push(p);
    }
    params.push(json!({"id":"blendMode","name":"Modo de mesclagem","type":"choice","min":null,"max":null,"default":"normal","animatable":false,
        "choices":BLENDS.iter().map(|(id,name)|json!({"id":id,"name":name})).collect::<Vec<_>>()}));
    for (id, name) in [("left", "Esquerda"), ("top", "Topo"), ("right", "Direita"), ("bottom", "Base")] {
        let mut p = spec(find_effect("crop").unwrap().param(id).unwrap());
        p["id"] = json!(format!("crop_{id}"));
        p["name"] = json!(format!("Recorte: {name}"));
        params.push(p);
    }
    items.push(json!({"id":"transform.motion","name":"Movimento","folder":["Ajustes"],"kind":"transform","phase":"3b","params":params,"processing":"mixed"}));
    items.push(json!({"id":"speed.clip","name":"Velocidade","folder":["Ajustes"],"kind":"speed","phase":"3b","processing":"mixed",
        "params":[{"id":"speed","name":"Velocidade","type":"float","min":0.25,"max":4,"default":1,"animatable":false,"unit":"×"}]}));
    items
}
fn spec(p: &ParamDef) -> Value {
    let mut spec = catalog::named_spec(p, &translated(p.label), translated);
    spec["animatable"] = json!(p.animatable);
    if track_choice(p) {
        spec["choices"].as_array_mut().unwrap().truncate(3);
    }
    if p.kind == filmcraft_project::ParamKind::Point && spec["default"].is_null() {
        // Corner Pin and gradient endpoints have engine-specific automatic positions. Omitting
        // those values preserves them; the contract's centre hint cannot describe a corner.
        if !["upper_left", "upper_right", "lower_left", "lower_right", "start", "end"].contains(&p.id) {
            spec["defaultFrom"] = json!("sourceCenter");
        } else {
            spec.as_object_mut().unwrap().remove("defaultFrom");
        }
    }
    if let Some(group) = p.group {
        spec["group"] = json!(translated(group));
    }
    spec
}

fn translated(name: &str) -> String {
    let pairs = [
        ("Evolution Options", "Opções de evolução"),
        ("Stabilization", "Estabilização"),
        ("Borders", "Bordas"),
        ("Advanced", "Avançado"),
        ("Matte Generation", "Geração da máscara"),
        ("Matte Cleanup", "Limpeza da máscara"),
        ("Spill Suppression", "Supressão de vazamento"),
        ("Color Correction", "Correção de cor"),
        ("(RGB) Gamma", "Gama RGB"),
        ("(RGB) Input Black Level", "Nível de preto de entrada RGB"),
        ("(RGB) Input White Level", "Nível de branco de entrada RGB"),
        ("(RGB) Output Black Level", "Nível de preto de saída RGB"),
        ("(RGB) Output White Level", "Nível de branco de saída RGB"),
        ("Action-Safe Margin", "Margem de segurança"),
        ("Additional Scale", "Escala adicional"),
        ("Alpha Blurriness", "Desfoque alfa"),
        ("Ambience Intensity", "Intensidade ambiente"),
        ("Ambience Light Color", "Cor da luz ambiente"),
        ("Amount", "Quantidade"),
        ("Amount of Noise", "Quantidade de ruído"),
        ("Amount to Tint", "Intensidade da tonalização"),
        ("Angle", "Ângulo"),
        ("Antialiasing for Best Quality", "Suavização de qualidade"),
        ("Blend", "Mistura"),
        ("Blend With Original", "Mistura com original"),
        ("Blending Mode", "Modo de mesclagem"),
        ("Blue Blurriness", "Desfoque azul"),
        ("Blur Amount", "Quantidade de desfoque"),
        ("Blur Dimensions", "Eixos de desfoque"),
        ("Blur Layer", "Trilha de desfoque"),
        ("Blur Length", "Comprimento do desfoque"),
        ("Blurriness", "Desfoque"),
        ("Border", "Borda"),
        ("Border Color", "Cor da borda"),
        ("Border Width", "Largura da borda"),
        ("Bottom", "Base"),
        ("Brightness", "Brilho"),
        ("Brush Size", "Tamanho do pincel"),
        ("Bump Height", "Altura do relevo"),
        ("Center", "Centro"),
        ("Center of Sphere", "Centro da esfera"),
        ("Channel", "Canal"),
        ("Choke", "Contração"),
        ("Clipping", "Limitar valores"),
        ("Color", "Cor"),
        ("Color Tolerance", "Tolerância de cor"),
        ("Colorize", "Colorir"),
        ("Complexity", "Complexidade"),
        ("Composite Using", "Compor usando"),
        ("Contrast", "Contraste"),
        ("Crop Less <-> Smooth More", "Recorte e suavização"),
        ("Curvature", "Curvatura"),
        ("Cutoff", "Corte"),
        ("Cycle (in Revolutions)", "Ciclo em voltas"),
        ("Cycle Evolution", "Repetir evolução"),
        ("Decay", "Decaimento"),
        ("Desaturate", "Dessaturar"),
        ("Detailed Analysis", "Análise detalhada"),
        ("Dimensions", "Dimensões"),
        ("Direction", "Direção"),
        ("Displacement", "Deslocamento"),
        ("Distance", "Distância"),
        ("Distance to Image", "Distância até a imagem"),
        ("Echoes", "Ecos"),
        ("Edge Color", "Cor das bordas"),
        ("Edge Feather", "Suavização das bordas"),
        ("Edge Sharpness", "Nitidez das bordas"),
        ("Edge Thin", "Afinar bordas"),
        ("Edge Threshold", "Limiar das bordas"),
        ("Edge Type", "Tipo de borda"),
        ("Edge Width", "Largura das bordas"),
        ("End Color", "Cor final"),
        ("Evolution", "Evolução"),
        ("Exposure", "Exposição"),
        ("Fade", "Esmaecer"),
        ("Feather", "Suavização da borda"),
        ("Flare Brightness", "Brilho do reflexo"),
        ("Flare Center", "Centro do reflexo"),
        ("Flare Color", "Cor do reflexo"),
        ("Focus", "Foco"),
        ("Focus Point", "Ponto de foco"),
        ("Focus Shape", "Forma do foco"),
        ("Focus Size", "Tamanho do foco"),
        ("Fractal Influence", "Influência fractal"),
        ("Frame Rate", "Quadros por segundo"),
        ("Framing", "Enquadramento"),
        ("Frequency", "Frequência"),
        ("Ghosts", "Reflexos secundários"),
        ("Glow Color", "Cor do brilho"),
        ("Glow Only", "Somente brilho"),
        ("Glow Radius", "Raio do brilho"),
        ("Gradient End", "Fim do gradiente"),
        ("Gradient Scatter", "Dispersão do gradiente"),
        ("Gradient Shape", "Forma do gradiente"),
        ("Gradient Start", "Início do gradiente"),
        ("Grain Size", "Tamanho do grão"),
        ("Green Blurriness", "Desfoque verde"),
        ("Hide Back Face", "Ocultar face traseira"),
        ("Highlight", "Realces"),
        ("Highlight Brightness", "Brilho dos realces"),
        ("Highlight Threshold", "Limiar dos realces"),
        ("Horizontal Blocks", "Blocos horizontais"),
        ("Horizontal Decentering", "Descentramento horizontal"),
        ("Hue", "Matiz"),
        ("Ignore Alpha", "Ignorar alfa"),
        ("Intensity", "Intensidade"),
        ("Invert", "Inverter"),
        ("Invert Alpha", "Inverter alfa"),
        ("Invert Blur", "Inverter desfoque"),
        ("Iris Rotation", "Rotação da íris"),
        ("Iris Shape", "Forma da íris"),
        ("Key Color", "Cor da chave"),
        ("Key Out Brights", "Remover claros"),
        ("Left", "Esquerda"),
        ("Length", "Comprimento"),
        ("Lens Type", "Tipo de lente"),
        ("Light Color", "Cor da luz"),
        ("Light Type", "Tipo de luz"),
        ("Lower Left", "Inferior esquerdo"),
        ("Lower Right", "Inferior direito"),
        ("Luma", "Luminância"),
        ("Luminance", "Luminância"),
        ("Magnification", "Ampliação"),
        ("Major Radius", "Raio maior"),
        ("Map Black To", "Substituir preto por"),
        ("Map White To", "Substituir branco por"),
        ("Mask Only", "Somente máscara"),
        ("Matte", "Máscara"),
        ("Maximum Blur", "Desfoque máximo"),
        ("Maximum Scale", "Escala máxima"),
        ("Method", "Método"),
        ("Mid Point", "Ponto médio"),
        ("Midpoint", "Ponto médio"),
        ("Minor Radius", "Raio menor"),
        ("Mode", "Modo"),
        ("Monochrome", "Monocromático"),
        ("Motion Blur", "Desfoque de movimento"),
        ("Offset (Turbulence)", "Deslocamento da turbulência"),
        ("Opacity", "Opacidade"),
        ("Output", "Saída"),
        ("Paint Surface", "Superfície de pintura"),
        ("Pedestal", "Pedestal"),
        ("Perspective", "Perspectiva"),
        ("Pinning", "Fixação"),
        ("Position", "Posição"),
        ("Position Amount", "Quantidade de deslocamento"),
        ("Preserve Scale", "Preservar escala"),
        ("Radius", "Raio"),
        ("Random Seed", "Semente aleatória"),
        ("Range", "Faixa"),
        ("Ray Color", "Cor dos raios"),
        ("Ray Length", "Comprimento dos raios"),
        ("Rays", "Raios"),
        ("Rays Only", "Somente raios"),
        ("Red Blurriness", "Desfoque vermelho"),
        ("Reflection Angle", "Ângulo do reflexo"),
        ("Reflection Center", "Centro do reflexo"),
        ("Relief", "Relevo"),
        ("Repeat Edge Pixels", "Repetir pixels da borda"),
        ("Replace Color", "Cor substituta"),
        ("Result", "Resultado"),
        ("Reverse", "Inverter sentido"),
        ("Right", "Direita"),
        ("Rotation", "Rotação"),
        ("Rotation Amount", "Quantidade de rotação"),
        ("Roundness", "Arredondamento"),
        ("Saturation", "Saturação"),
        ("Scale", "Escala"),
        ("Scale Amount", "Quantidade de escala"),
        ("Setting", "Configuração"),
        ("Shadow", "Sombra"),
        ("Shadow Color", "Cor da sombra"),
        ("Shadow Only", "Somente sombra"),
        ("Shape", "Forma"),
        ("Sharp Colors", "Cores nítidas"),
        ("Sharpen Amount", "Intensidade de nitidez"),
        ("Shift Center To", "Mover centro para"),
        ("Show Focus Area", "Mostrar área de foco"),
        ("Similarity", "Semelhança"),
        ("Size", "Tamanho"),
        ("Smoothness", "Suavidade"),
        ("Soften", "Suavizar"),
        ("Softness", "Suavidade"),
        ("Solid Colors", "Cores sólidas"),
        ("Source Point", "Ponto de origem"),
        ("Speed", "Velocidade"),
        ("Spill", "Vazamento de cor"),
        ("Spread", "Dispersão"),
        ("Start Color", "Cor inicial"),
        ("Stretch Map to Fit", "Ajustar mapa ao quadro"),
        ("Stretch Width or Height", "Esticar largura ou altura"),
        ("Strobe Color", "Cor do estrobo"),
        ("Strobe Duration (secs)", "Duração do estrobo (s)"),
        ("Strobe Period (secs)", "Período do estrobo (s)"),
        ("Stroke Angle", "Ângulo do traço"),
        ("Stroke Density", "Densidade dos traços"),
        ("Stroke Length", "Comprimento do traço"),
        ("Stroke Only", "Somente traço"),
        ("Stroke Randomness", "Aleatoriedade dos traços"),
        ("Surface Gloss", "Brilho da superfície"),
        ("Surface Material", "Material da superfície"),
        ("Swivel", "Giro horizontal"),
        ("Target Color", "Cor alvo"),
        ("Threshold", "Limiar"),
        ("Tilt", "Inclinação"),
        ("Tolerance", "Tolerância"),
        ("Top", "Topo"),
        ("Transparency", "Transparência"),
        ("Twirl Center", "Centro do redemoinho"),
        ("Twirl Radius", "Raio do redemoinho"),
        ("Upper Left", "Superior esquerdo"),
        ("Upper Right", "Superior direito"),
        ("Use Color Noise", "Ruído colorido"),
        ("Use Glow Color", "Usar cor do brilho"),
        ("Vertical Blocks", "Blocos verticais"),
        ("Vertical Decentering", "Descentramento vertical"),
        ("Wave Height", "Altura da onda"),
        ("Wave Speed", "Velocidade da onda"),
        ("Wave Width", "Largura da onda"),
        ("White Is High", "Branco representa alto"),
        ("Width", "Largura"),
        ("X Rotation", "Rotação X"),
        ("Y Rotation", "Rotação Y"),
        ("Z Position", "Posição Z"),
        ("Z Rotation", "Rotação Z"),
        ("Zoom Amount", "Quantidade de zoom"),
        ("105mm Prime", "Fixa 105 mm"),
        ("35mm Prime", "Fixa 35 mm"),
        ("50-300mm Zoom", "Zoom 50–300 mm"),
        ("Add", "Adicionar"),
        ("Aggressive", "Agressivo"),
        ("Alpha", "Alfa"),
        ("Alpha Channel", "Canal alfa"),
        ("Anamorphic", "Anamórfica"),
        ("Blue", "Azul"),
        ("Bulge", "Abaular"),
        ("Bulge Smoother", "Abaular suavizado"),
        ("Circle", "Círculo"),
        ("Color Channel", "Canal de cor"),
        ("Composite", "Composição"),
        ("Cross Displacement", "Deslocamento cruzado"),
        ("Custom", "Personalizado"),
        ("Cut", "Corte"),
        ("Default", "Padrão"),
        ("Diamond", "Losango"),
        ("Directional", "Direcional"),
        ("Green", "Verde"),
        ("Hexagon", "Hexágono"),
        ("High", "Alta"),
        ("Horizontal", "Horizontal"),
        ("Horizontal Displacement", "Deslocamento horizontal"),
        ("Horizontal and Vertical", "Horizontal e vertical"),
        ("Inside", "Dentro"),
        ("Linear", "Linear"),
        ("Low", "Baixa"),
        ("Matte Alpha", "Alfa da máscara"),
        ("Matte Luma", "Luminância da máscara"),
        ("Multiply", "Multiplicar"),
        ("No Motion", "Sem movimento"),
        ("None", "Nenhum"),
        ("Normal", "Normal"),
        ("Octagon", "Octógono"),
        ("Omni", "Omnidirecional"),
        ("Outside", "Fora"),
        ("Overlay", "Sobrepor"),
        ("Paint On Black", "Pintar sobre preto"),
        ("Paint On Original Image", "Pintar sobre original"),
        ("Paint On Transparent", "Pintar sobre transparente"),
        ("Paint On White", "Pintar sobre branco"),
        ("Pentagon", "Pentágono"),
        ("Photocopy", "Fotocópia"),
        ("Photocopy Color", "Fotocópia colorida"),
        ("Pin All", "Fixar tudo"),
        ("Pin Horizontal", "Fixar horizontal"),
        ("Pin Vertical", "Fixar vertical"),
        ("Position, Scale, Rotation", "Posição, escala e rotação"),
        ("Radial", "Radial"),
        ("Red", "Vermelho"),
        ("Reflected", "Refletido"),
        ("Relaxed", "Suave"),
        ("Roughen", "Rugoso"),
        ("Roughen Color", "Rugoso colorido"),
        ("Rusty", "Enferrujado"),
        ("Rusty Color", "Enferrujado colorido"),
        ("Screen", "Trama"),
        ("Smooth Motion", "Suavizar movimento"),
        ("Spiky", "Pontudo"),
        ("Spotlight", "Luz focal"),
        ("Square", "Quadrado"),
        ("Stabilize Only", "Somente estabilizar"),
        ("Stabilize, Crop", "Estabilizar e recortar"),
        ("Stabilize, Crop, Auto-scale", "Estabilizar, recortar e ajustar escala"),
        ("Stabilize, Synthesize Edges", "Estabilizar e reconstruir bordas"),
        ("Subspace Warp", "Deformação espacial"),
        ("Triangle", "Triângulo"),
        ("Turbulent", "Turbulento"),
        ("Turbulent Smoother", "Turbulento suavizado"),
        ("Twist", "Torcer"),
        ("Twist Smoother", "Torcer suavizado"),
        ("Vertical", "Vertical"),
        ("Vertical Displacement", "Deslocamento vertical"),
    ];
    if let Some((_, value)) = pairs.iter().find(|(key, _)| *key == name) {
        return (*value).into();
    }
    for (en, pt) in [("Color ", "Cor "), ("Point ", "Ponto "), ("Video ", "Vídeo "), ("Light ", "Luz ")] {
        if let Some(n) = name.strip_prefix(en)
            && n.parse::<u32>().is_ok()
        {
            return format!("{pt}{n}");
        }
    }
    if name.contains('-') {
        return name.split('-').map(|s| if s == "Const" { "Constante".into() } else { translated(s) }).collect::<Vec<_>>().join(" / ");
    }
    if name == "RGB" || name.parse::<u32>().is_ok() {
        return name.into();
    }
    panic!("tradução ausente: {name}")
}
