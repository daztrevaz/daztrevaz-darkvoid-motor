//! Catálogo de áudio, adaptação dos efeitos e automação de ducking do engine.
use crate::{
    catalog,
    effects::Effect,
    protocol::{Result, error},
};
use filmcraft_engine::{MediaPool, Session};
use filmcraft_project::{EffectInstance, Param, ParamDef, ParamValue, Project, find_effect};
use filmcraft_time::Tick;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
};

pub const EFFECTS: &[(&str, &str, &str)] = &[
    ("denoise", "Redução de ruído", "Limpeza de voz"),
    ("dehummer", "Removedor de chiado elétrico", "Limpeza de voz"),
    ("speech_enhance", "Realce de voz", "Limpeza de voz"),
    ("parametric_eq", "Equalizador paramétrico", "Filtros e equalização"),
    ("graphic_eq", "Equalizador gráfico de 10 bandas", "Filtros e equalização"),
    ("highpass", "Passa-altas", "Filtros e equalização"),
    ("lowpass", "Passa-baixas", "Filtros e equalização"),
    ("notch", "Filtro notch", "Filtros e equalização"),
    ("dynamics_rack", "Dinâmica: compressor, limitador e gate", "Dinâmica"),
    ("single_band_compressor", "Compressor", "Dinâmica"),
    ("multiband_compressor", "Compressor multibanda", "Dinâmica"),
    ("hard_limiter", "Limitador rígido", "Dinâmica"),
    ("deesser", "De-esser", "Limpeza de voz"),
    ("amplify", "Amplificar", "Volume e estéreo"),
    ("bass", "Graves", "Filtros e equalização"),
    ("treble", "Agudos", "Filtros e equalização"),
    ("stereo_width", "Largura estéreo", "Volume e estéreo"),
    ("balance_a", "Balanço", "Volume e estéreo"),
];

fn parameters(id: &str) -> Vec<ParamDef> {
    let mut params = find_effect(id).expect("efeito de áudio aprovado").params.clone();
    // O DSP tem look-ahead fixo de 5 ms e faixas menores que a definição histórica.
    // Só anuncia controles efetivos, sem aceitar silenciosamente valores ignorados.
    if id == "hard_limiter" {
        params.retain(|p| p.id != "lookahead");
        for p in &mut params {
            if let filmcraft_project::ParamKind::Float { min, max, .. } = &mut p.kind {
                if p.id == "boost" {
                    *min = 0.0;
                    *max = 24.0;
                }
                if p.id == "max" {
                    *min = -24.0;
                }
            }
        }
    }
    params
}
pub fn prepare(inputs: &[Effect], clip: &str) -> Result<Vec<EffectInstance>> {
    let mut instances = HashSet::new();
    let mut effects = Vec::new();
    for input in inputs {
        if input.instance_id.trim().is_empty() || !instances.insert(&input.instance_id) {
            return Err(catalog::invalid(clip, "audioEffects.instanceId"));
        }
        let id = input.id.strip_prefix("audio.").filter(|id| EFFECTS.iter().any(|e| e.0 == *id)).ok_or_else(|| catalog::invalid(clip, "audioEffects.id"))?;
        let params = parameters(id);
        let mut effect = find_effect(id).expect("efeito de áudio").instance();
        effect.enabled = input.enabled;
        for (id, value) in &input.params {
            let field = format!("audioEffects.{}.params.{id}", input.instance_id);
            let p = params.iter().find(|p| p.id == id).ok_or_else(|| catalog::invalid(clip, &field))?;
            effect.params.insert(id.clone(), crate::effects::animated(p, value, clip, &field)?);
        }
        effects.push(effect);
    }
    Ok(effects)
}

pub fn items() -> Vec<Value> {
    let mut items: Vec<_> = EFFECTS
        .iter()
        .map(|(id, name, folder)| {
            let params: Vec<_> = parameters(id)
                .iter()
                .map(|p| {
                    let mut spec = catalog::named_spec(p, &translated(p.label), translated);
                    spec["animatable"] = json!(p.animatable);
                    if let Some(group) = p.group {
                        spec["group"] = json!(translated(group));
                    }
                    spec
                })
                .collect();
            json!({"id":format!("audio.{id}"),"name":name,"folder":["Áudio",folder],"kind":"audioEffect","phase":"4","params":params,"processing":"cpu"})
        })
        .collect();
    items.push(json!({"id":"audio.duck","name":"Abaixar música durante a voz","folder":["Áudio","Trilhas"],"kind":"duck","phase":"4","processing":"cpu",
        "params":[
            {"id":"by","name":"Trilhas de voz","type":"choice","min":null,"max":null,"default":null,"animatable":false,
                "choices":[{"id":"V1","name":"Vídeo 1"},{"id":"V2","name":"Vídeo 2"},{"id":"A1","name":"Áudio 1"},{"id":"A2","name":"Áudio 2"}]},
            {"id":"reduce","name":"Redução","type":"float","min":0,"max":40,"default":12,"unit":"dB","animatable":false},
            {"id":"sensitivity","name":"Sensibilidade","type":"float","min":0,"max":100,"default":50,"unit":"%","animatable":false},
            {"id":"fade","name":"Duração das rampas","type":"float","min":0,"max":3600,"default":0.8,"unit":"s","animatable":false}]}));
    items
}
fn translated(name: &str) -> String {
    let label = match name {
        "Amount" => "Intensidade",
        "Frequency" => "Frequência",
        "Gain" => "Ganho",
        "Number of Harmonics" => "Número de harmônicos",
        "Q" => "Q",
        "Mix" => "Mistura",
        "Voice" => "Voz",
        "Low Tone" => "Grave",
        "High Tone" => "Aguda",
        "Master Gain" => "Ganho geral",
        "On" => "Ligado",
        "Slope" => "Inclinação",
        "High Pass" => "Passa-altas",
        "Low Pass" => "Passa-baixas",
        "Low Shelf" => "Prateleira de graves",
        "High Shelf" => "Prateleira de agudos",
        "Cutoff" => "Frequência de corte",
        "Notch Width" => "Largura do corte",
        "Narrow" => "Estreita",
        "Very Narrow" => "Muito estreita",
        "Super Narrow" => "Superestreita",
        "Auto Gate" => "Gate automático",
        "Threshold" => "Limiar",
        "Attack" => "Ataque",
        "Release" => "Soltura",
        "Hold" => "Sustentação",
        "Compressor" => "Compressor",
        "Ratio" => "Proporção",
        "Auto Makeup" => "Compensação automática",
        "Makeup" => "Compensação de ganho",
        "Expander" => "Expansor",
        "Limiter" => "Limitador",
        "Soft Clip" => "Saturação suave",
        "Output Gain" => "Ganho de saída",
        "Crossovers" => "Divisões de frequência",
        "Low" => "Graves",
        "Mid" => "Médios",
        "High" => "Agudos",
        "Solo" => "Isolar",
        "Bypass" => "Desativar",
        "Output" => "Saída",
        "Limiter Threshold" => "Limiar do limitador",
        "Limiter Release" => "Soltura do limitador",
        "Link Channels" => "Vincular canais",
        "Maximum Amplitude" => "Amplitude máxima",
        "Input Boost" => "Ganho de entrada",
        "Release Time" => "Tempo de soltura",
        "Maximum Reduction" => "Redução máxima",
        "Boost" => "Reforço",
        "Width" => "Largura",
        "Balance" => "Balanço",
        _ if name.starts_with("Band ") => return name.replacen("Band ", "Banda ", 1),
        _ if name.starts_with("Notch ") => return name.replacen("Notch ", "Corte ", 1),
        _ if name.ends_with(" Hz") || name.ends_with(" kHz") || name.ends_with(" dB/oct") => return name.to_owned(),
        _ => panic!("parâmetro de áudio sem tradução: {name}"),
    };
    label.into()
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TrackAudio {
    pub duck: Option<Duck>,
}
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Duck {
    by: Vec<String>,
    #[serde(default = "reduce")]
    reduce: f64,
    #[serde(default = "sensitivity")]
    sensitivity: f64,
    #[serde(default = "fade")]
    fade: f64,
}
fn reduce() -> f64 {
    12.0
}
fn sensitivity() -> f64 {
    50.0
}
fn fade() -> f64 {
    0.8
}
pub type Tracks = BTreeMap<String, TrackAudio>;
fn index(name: &str) -> Option<usize> {
    ["V1", "V2", "A1", "A2"].iter().position(|n| *n == name)
}
pub fn validate_tracks(tracks: &Tracks) -> Result<()> {
    for (name, track) in tracks {
        if !["A1", "A2"].contains(&name.as_str()) {
            return Err(catalog::invalid(name, "tracks"));
        }
        if let Some(duck) = &track.duck {
            let mut seen = HashSet::new();
            if !duck.reduce.is_finite()
                || !(0.0..=40.0).contains(&duck.reduce)
                || !duck.sensitivity.is_finite()
                || !(0.0..=100.0).contains(&duck.sensitivity)
                || !duck.fade.is_finite()
                || !(0.0..=3600.0).contains(&duck.fade)
                || duck.by.iter().any(|by| index(by).is_none() || by == name || !seen.insert(by))
            {
                return Err(catalog::invalid(name, "tracks.duck"));
            }
        }
    }
    Ok(())
}

pub fn apply_duck(session: &mut Session, tracks: &Tracks) -> Result<()> {
    let sequence = session.state.active_sequence.ok_or_else(|| error("no_session", "Abra uma sessão antes de aplicar ducking."))?;
    let duration = session.active_sequence().expect("sequência ativa").duration();
    // Cada análise usa o mesmo estado sem ducking: nenhuma dependência circular entre trilhas.
    let mut base = (*session.project).clone();
    for track in &mut base.sequence_mut(sequence).expect("sequência ativa").audio_tracks {
        track.mixer.lanes.remove("volume");
    }
    let mut plans = Vec::new();
    for (name, track) in tracks {
        let Some(duck) = &track.duck else { continue };
        let mut project = base.clone();
        let seq = project.sequence_mut(sequence).expect("sequência ativa");
        for (i, track) in seq.audio_tracks.iter_mut().enumerate() {
            track.muted = !duck.by.iter().any(|by| index(by) == Some(i));
        }
        let project = Arc::new(project);
        let pool = Arc::new(MediaPool::default());
        let sources = pool.provider(project.clone(), session.services.clone());
        let rate = 48000;
        let hop = 480;
        let end = duration.to_units_floor(rate);
        let mut envelope = Vec::new();
        let mut pos = 0;
        while pos < end {
            let count = (end - pos).min(rate) as usize;
            let samples = filmcraft_render::audio::mix_sequence(
                &project,
                project.sequence(sequence).expect("sequência ativa"),
                pos - 3 * hop,
                count + 6 * hop as usize,
                &sources,
            );
            let refs: Vec<_> = samples.channels.iter().map(Vec::as_slice).collect();
            let block = filmcraft_audio_dsp::ducking::envelope_db(&refs, rate as f32, 0.01, 0.05);
            envelope.extend(block.into_iter().skip(3).take(count.div_ceil(hop as usize)));
            pos += count as i64;
        }
        let threshold = filmcraft_audio_dsp::ducking::sensitivity_threshold_db((duck.sensitivity / 10.0) as f32);
        let regions = filmcraft_audio_dsp::ducking::activity(&envelope, 0.01, threshold, 0.1, 0.25);
        let keys = filmcraft_audio_dsp::ducking::duck_keyframes(&regions, 0.0, duration.seconds(), 0.0, duck.reduce, duck.fade);
        let mut lane = Param::new(ParamValue::Float(0.0));
        // Antes/depois das regiões o nível deve voltar a zero, inclusive fora da primeira rampa.
        lane.put_keyframe(Tick::ZERO, ParamValue::Float(keys.first().filter(|(t, _)| *t == 0.0).map_or(0.0, |(_, v)| *v)));
        for (time, level) in keys {
            lane.put_keyframe(Tick::from_seconds_f64(time), ParamValue::Float(level));
        }
        lane.put_keyframe(duration, ParamValue::Float(lane.keyframes.last().and_then(|k| k.value.as_f64()).unwrap_or(0.0)));
        plans.push((index(name).expect("trilha validada"), lane));
    }
    let project: &mut Project = Arc::make_mut(&mut session.project);
    let seq = project.sequence_mut(sequence).expect("sequência ativa");
    for track in &mut seq.audio_tracks {
        track.mixer.lanes.remove("volume");
    }
    for (index, lane) in plans {
        seq.audio_tracks[index].mixer.lanes.insert("volume".into(), lane);
    }
    Ok(())
}
