"""Gera os anexos públicos a partir da execução física M3.0 (somente biblioteca padrão)."""
import json
import hashlib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
raw = json.loads((ROOT / "target/m3.0-medicoes.json").read_text(encoding="utf-8"))
assert raw["performance"], "A medição física ainda não terminou."
by_id = {row["id"]: row for row in raw["cases"]}
defs = {row["id"]: row for row in raw["catalog"]}

MACHINE = {
    "date": "2026-11-16",
    "baseCommit": "fbb371d3f16e16193a71875ec31803d0e0232de6",
    "os": "Windows 11 Pro 10.0.26300",
    "cpu": "AMD Ryzen 5 5600X (6 cores / 12 logical processors)",
    "ramBytes": 34279698432,
    "gpu": "NVIDIA GeForce RTX 3060 Ti",
    "vramMiB": 8192,
    "driver": "610.88",
    "rust": "1.99.0 (b940084d7), LLVM 23.1.1, x86_64-pc-windows-msvc",
}

def write(name, data):
    options = {"ensure_ascii": False, "allow_nan": False}
    if name == "m3.0-medicoes.json":
        # Um caso por linha mantém o inventário completo sem um diff de 100 mil linhas.
        header = {k: v for k, v in data.items() if k != "cases"}
        text = json.dumps(header, indent=2, **options)[:-2]
        rows = ",\n".join("    " + json.dumps(row, separators=(",", ":"), **options) for row in data["cases"])
        text += ',\n  "cases": [\n' + rows + "\n  ]\n}\n"
    else:
        text = json.dumps(data, indent=2, **options) + "\n"
    (ROOT / "docs" / name).write_text(text, encoding="utf-8")

report = {k: v for k, v in raw.items() if k not in ("catalog", "presets")}
report["machine"] = MACHINE
report["harnessSha256"] = {
    name: hashlib.sha256((ROOT / name).read_bytes()).hexdigest()
    for name in ("crates/darkvoid-motor/src/measurement.rs", "crates/darkvoid-motor/src/preview.rs", "crates/darkvoid-motor/Cargo.toml", "Cargo.lock")
}
report["presets"] = raw["presets"]
write("m3.0-medicoes.json", report)

LABELS = {
    "basic_on": "Correção básica", "input_lut": "LUT de entrada", "temperature": "Temperatura", "tint": "Matiz verde e magenta",
    "exposure": "Exposição", "contrast": "Contraste", "highlights": "Realces", "shadows": "Sombras", "whites": "Brancos", "blacks": "Pretos",
    "saturation": "Saturação", "creative_on": "Criativo", "look": "Look", "look_lut": "LUT criativa", "look_intensity": "Intensidade",
    "faded_film": "Filme desbotado", "sharpen": "Nitidez", "vibrance": "Vivacidade", "creative_sat": "Saturação criativa",
    "shadow_tint": "Coloração das sombras", "highlight_tint": "Coloração dos realces", "curves_on": "Curvas", "curve_luma": "Curva de luminância",
    "curve_red": "Curva vermelha", "curve_green": "Curva verde", "curve_blue": "Curva azul", "hue_vs_sat": "Matiz por saturação",
    "hue_vs_hue": "Matiz por matiz", "hue_vs_luma": "Matiz por luminância", "luma_vs_sat": "Luminância por saturação", "sat_vs_sat": "Saturação por saturação",
    "wheels_on": "Rodas de cor", "wheel_shadows": "Roda das sombras", "wheel_shadows_l": "Luz das sombras",
    "wheel_midtones": "Roda dos meios-tons", "wheel_midtones_l": "Luz dos meios-tons", "wheel_highlights": "Roda dos realces", "wheel_highlights_l": "Luz dos realces",
    "hsl_on": "Correção secundária HSL", "hsl_hue": "Matiz central", "hsl_hue_range": "Faixa de matiz", "hsl_sat_min": "Saturação mínima",
    "hsl_luma_min": "Luminância mínima", "hsl_luma_max": "Luminância máxima", "hsl_soft": "Suavização da seleção", "hsl_show_mask": "Visualizar seleção",
    "hsl_denoise": "Redução de ruído da seleção", "hsl_blur": "Desfoque da seleção", "hsl_temp": "Temperatura secundária", "hsl_tint": "Matiz secundário",
    "hsl_sat": "Saturação secundária", "hsl_hue_shift": "Deslocamento de matiz", "vignette_on": "Vinheta", "vignette_amount": "Intensidade da vinheta",
    "vignette_midpoint": "Ponto médio", "vignette_roundness": "Arredondamento", "vignette_feather": "Suavidade da vinheta",
    "direction": "Direção", "motion_blur": "Desfoque de movimento", "angle": "Ângulo", "feather": "Suavidade", "border_width": "Largura da borda",
    "border_color": "Cor da borda", "antialias": "Suavização de bordas", "blur": "Desfoque", "color": "Cor", "hold": "Permanência",
    "blurriness": "Desfoque", "dimensions": "Dimensões", "repeat_edge": "Repetir pixels da borda", "length": "Comprimento",
    "amount": "Intensidade", "radius": "Raio", "threshold": "Limiar", "position": "Posição", "scale": "Escala", "scale_width": "Escala horizontal",
    "uniform_scale": "Escala uniforme", "rotation": "Rotação", "anchor": "Ponto de ancoragem", "anti_flicker": "Redução de cintilação",
    "opacity": "Opacidade", "blend": "Mesclagem", "sharpness": "Nitidez de saída", "method": "Método", "speed": "Velocidade",
}
CHOICES = {
    "None": "Nenhum", "Teal & Orange": "Azul-petróleo e laranja", "Warm Film": "Filme quente", "Cool Blue": "Azul frio", "Bleach Bypass": "Prata retida",
    "Faded Matte": "Mate desbotado", "Monochrome": "Monocromático", "Golden Hour": "Hora dourada", "Night": "Noite",
    "Horizontal and Vertical": "Horizontal e vertical", "Horizontal": "Horizontal", "Vertical": "Vertical",
    "From North": "De cima", "From East": "Da direita", "From South": "De baixo", "From West": "Da esquerda",
    "Off": "Desligado", "Low": "Baixa", "Medium": "Média", "High": "Alta", "Color/Gray": "Cor e cinza", "Color/Black": "Cor e preto", "White/Black": "Branco e preto",
}
GROUPS = {"Basic Correction": "Básico", "Creative": "Criativo", "Curves": "Curvas", "Color Wheels & Match": "Rodas de cor", "HSL Secondary": "HSL secundária", "Vignette": "Vinheta"}

def parameter(p):
    kind, default = p["kind"], p["default"]
    default = next(iter(default.values())) if isinstance(default, dict) else default
    result = {"id": p["id"], "name": LABELS[p["id"]], "type": "float", "min": None, "max": None, "default": default, "animatable": p["animatable"]}
    if isinstance(kind, dict) and "Float" in kind:
        result.update(min=kind["Float"]["min"], max=kind["Float"]["max"], unit=kind["Float"]["unit"])
    elif isinstance(kind, dict) and "Choice" in kind:
        result.update(type="choice", choices=[{"id": str(i), "name": CHOICES.get(s, s)} for i, s in enumerate(kind["Choice"])])
        result["default"] = str(default)
    elif isinstance(kind, dict) and "Curve" in kind:
        result.update(type="curve", min=0, max=1, hue=kind["Curve"]["hue"])
    else:
        result["type"] = {"Point": "point", "Color": "color", "Bool": "bool", "Angle": "float", "Text": "lut", "Wheel": "wheel"}[kind]
        if kind in ("Color", "Wheel"):
            result.update(min=-1 if kind == "Wheel" else 0, max=1)
        if kind == "Angle":
            result.update(min=-3600, max=3600, unit="graus")
        if kind in ("Point", "Wheel"):
            result["default"] = [default["x"], default["y"]]
            if result["default"] == [None, None]:
                result["default"] = None
                result["defaultFrom"] = "frameCenter" if p["id"] == "position" else "sourceCenter"
    if p.get("group"):
        result["group"] = GROUPS[p["group"]]
    return result

def status(test):
    if test not in by_id:
        return "pending"
    row = by_id[test]
    return "measured" if row["pass"] and row.get("observable") else "blocked" if not row["pass"] else "pending"

items = []
def add(engine, public, name, folder, phase, test=None):
    definition = defs[engine]
    params = [parameter(p) for p in definition["params"] if p["id"] in LABELS]
    if engine == "lumetri":
        params = [p for p in params if p["id"] not in ("look", "look_lut", "look_intensity")]
        for p in params:
            p["measurementCase"] = "lut.cube" if p["id"] == "input_lut" else "color." + p["id"]
            p["status"] = status(p["measurementCase"])
    if engine == "opacity":
        params = [p for p in params if p["id"] != "blend"]
    items.append({"id": public, "engineId": engine, "name": name, "folder": folder, "phase": phase, "params": params,
                  "measurementCase": test or engine, "status": status(test or engine), "processing": "mixed"})

for engine, name, folder in [
    ("cross_dissolve", "Dissolução cruzada", "Dissoluções"), ("dip_to_black", "Passagem pelo preto", "Dissoluções"),
    ("dip_to_white", "Passagem pelo branco", "Dissoluções"), ("dip_to_color", "Passagem por cor", "Dissoluções"),
    ("blur_dissolve", "Dissolução com desfoque", "Dissoluções"), ("push", "Empurrar", "Deslocamentos"),
    ("slide", "Deslizar", "Deslocamentos"), ("linear_wipe", "Varredura linear", "Varreduras")]:
    add(engine, "transition." + engine, name, ["Transições", folder], "3a")
add("lumetri", "color.grade", "Cor", ["Cor"], "3a", "color.contrast")
for engine, name, folder in [
    ("gaussian_blur", "Desfoque gaussiano", "Desfoque e nitidez"), ("directional_blur", "Desfoque direcional", "Desfoque e nitidez"),
    ("sharpen", "Nitidez", "Desfoque e nitidez"), ("unsharp_mask", "Máscara de nitidez", "Desfoque e nitidez"),
    ("black_white", "Preto e branco", "Estilização"), ("horizontal_flip", "Inverter horizontalmente", "Transformação"),
    ("vertical_flip", "Inverter verticalmente", "Transformação")]:
    add(engine, "effect." + engine, name, ["Efeitos", folder], "3b")
add("motion", "transform.motion", "Transformação", ["Movimento"], "3b", "transform.scale")
items[-1]["status"] = "conditional"
for p in items[-1]["params"]:
    p["measurementCase"] = "transform." + p["id"]
    p["status"] = status(p["measurementCase"])
add("opacity", "transform.opacity", "Opacidade", ["Movimento"], "3b", "transform.opacity")

LOOKS = [
    ("teal-orange", "Azul-petróleo e laranja"), ("warm-film", "Filme quente"), ("cool-blue", "Azul frio"), ("silver-retention", "Prata retida"),
    ("faded-matte", "Mate desbotado"), ("monochrome", "Monocromático"), ("golden-hour", "Hora dourada"), ("night", "Noite")]
intensity = {"id": "intensity", "name": "Intensidade", "type": "float", "min": 0, "max": 100, "default": 100, "animatable": False, "unit": "%"}
for i, (stable, name) in enumerate(LOOKS, 1):
    items.append({"id": "look." + stable, "name": name, "folder": ["Looks", "Criativos"], "phase": "3a", "params": [intensity],
                  "engineLookChoice": i, "measurementCase": f"look.{i}", "status": status(f"look.{i}"),
                  "limitation": "Paridade medida com intensidade 100; controle global ainda sujeito a I3.1/M3.2."})
PRESETS = [
    ("teal-shadows-warm-skin", "Sombras azul-petróleo, pele quente"), ("night-exterior", "Exterior noturno"), ("golden-dusk", "Entardecer dourado"),
    ("steel-blue", "Azul aço"), ("hard-sun", "Sol intenso"), ("faded-print", "Cópia desbotada"), ("silver-print", "Cópia com prata retida"),
    ("cross-process", "Revelação cruzada"), ("warm-negative", "Negativo quente"), ("neutral-mono", "Monocromático neutro"), ("hard-mono", "Monocromático intenso"),
    ("sepia", "Sépia"), ("cold-mono", "Monocromático frio"), ("lift-shadows", "Sombras abertas"), ("protect-highlights", "Realces preservados"),
    ("half-saturation", "Meia saturação"), ("boost-vibrance", "Vivacidade reforçada"), ("legal-range", "Faixa de sinal reduzida")]
for preset, (stable, name) in zip(raw["presets"], PRESETS, strict=True):
    items.append({"id": "look.preset." + stable, "name": name,
                  "folder": ["Looks", {"Cinematic": "Cinema", "Film Emulation": "Filme", "Monochrome": "Monocromáticos", "Technical": "Técnicos"}[preset["folder"]]],
                  "phase": "3a", "params": [intensity], "enginePreset": preset["name"], "measurementCase": preset["id"], "status": status(preset["id"]),
                  "limitation": "Preset completo medido; mistura global por intensidade ainda precisa de adaptação e medição."})
items.extend([
    {"id": "clip.speed", "name": "Velocidade", "folder": ["Movimento"], "phase": "3b", "status": "conditional",
     "params": [{"id": "speed", "name": "Velocidade", "type": "float", "min": 0.25, "max": 4, "default": 1, "animatable": False}]},
    {"id": "export.classic", "name": "Clássico", "folder": ["Exportação", "Escala"], "phase": "3b", "status": "blocked",
     "reason": "Engine atual amplia por bilinear; filtro de alta qualidade e nitidez após a escala ainda não existem nesse caminho.",
     "params": [{"id": "method", "name": "Método", "type": "choice", "min": None, "max": None, "default": "classic", "choices": [{"id": "classic", "name": "Clássico"}], "animatable": False},
                {"id": "sharpness", "name": "Nitidez de saída", "type": "float", "min": 0, "max": 100, "default": 0, "animatable": False, "unit": "%"}]},
])
for item in items:
    item["kind"] = (
        "transition" if item["id"].startswith("transition.") else "color" if item["id"].startswith("color.") else
        "look" if item["id"].startswith("look.") else "transform" if item["id"].startswith("transform.") else
        "speed" if item["id"] == "clip.speed" else "scale" if item["id"] == "export.classic" else "effect"
    )
    item.setdefault("processing", "mixed")
write("m3.0-catalogo.json", {"draft": True, "version": 3, "note": "Proposta para I3.1; não é resposta implementada de catalog.get. Status measured não valida todo o domínio de parâmetros.", "items": items})

summary = []
summary.append("## Resultados observados\n")
summary.append("Resultados brutos em [m3.0-medicoes.json](m3.0-medicoes.json); proposta completa de nomes, pastas e parâmetros em [m3.0-catalogo.json](m3.0-catalogo.json).\n")
summary.append("| Grupo | Casos | Paridade aprovada | Mudança visível | Falhas de execução |\n| --- | ---: | ---: | ---: | ---: |")
for kind in sorted({c["kind"] for c in raw["cases"]}):
    rows = [c for c in raw["cases"] if c["kind"] == kind]
    summary.append(f"| {kind} | {len(rows)} | {sum(c['pass'] for c in rows)} | {sum(c.get('observable', False) for c in rows)} | {sum('error' in c for c in rows)} |")
summary.append("\nUma aprovação aqui é de paridade dos quadros amostrados. Mudança visível é condição adicional; nem todo caso aprovado deve entrar no produto.\n")
summary.append("### Desempenho\n\n| Resolução | Render | Prévia com JPEG (qps) | Render bruto mediana/p95 (ms) | Entrega mediana/p95 (ms) | MP4 de 1 s (s) | Codificador |\n| --- | --- | ---: | --- | --- | ---: | --- |")
for p in raw["performance"]:
    summary.append(f"| {p['size'][0]} × {p['size'][1]} | {p['path']} | {p['previewFps']:.2f} | {p['previewRawMs']['median']:.2f} / {p['previewRawMs']['p95']:.2f} | {p['previewDeliveryMs']['median']:.2f} / {p['previewDeliveryMs']['p95']:.2f} | {p['export']['seconds']:.3f} | {p['export']['encoder'] or 'software'} |")
summary.append("\n### Fila real da prévia\n\n| Resolução | Pedidos | JPEGs recebidos em 2 s | Qps recebidos | Descartes contabilizados |\n| --- | ---: | ---: | ---: | ---: |")
for p in raw["performance"]:
    stream = p.get("streaming")
    if stream:
        summary.append(f"| {p['size'][1]}p | {stream['submitted']} | {stream['received']} | {stream['fps']:.2f} | {stream['dropped']} |")
summary.append("\nFila, compositor e três encoders JPEG reais; recepção em `watch`, sem WebSocket, canvas ou áudio. Amostra de 2 s; não comprova reprodução contínua de uma hora.\n")
if raw.get("upscaleExport2160"):
    up = raw["upscaleExport2160"]
    summary.append(f"### Escala real de exportação 1080p para 2160p\n\nUm quadro PNG, sequência 1080p, `ExportSettings.frame_size=(3840,2160)`: CPU {up['cpuExport']['seconds']:.3f} s; GPU {up['gpuExport']['seconds']:.3f} s. Erro GPU/CPU: p99 {up['gpuVsCpu']['p99']}, média {up['gpuVsCpu']['meanMaxRgb']:.3f}, máximo {up['gpuVsCpu']['max']}. Filtro atual bilinear; não aprova M3.4.\n")
summary.append("\n| Resolução/rota | Preparar sinal mediana/p95 (ms) | Forma de onda mediana/p95 (ms) | Vetorscópio mediana/p95 (ms) |\n| --- | --- | --- | --- |")
for p in raw["performance"]:
    s = p["scopes"]
    cells = [f"{s[k]['median']:.3f} / {s[k]['p95']:.3f}" for k in ("signalMs", "waveformMs", "vectorscopeMs")]
    summary.append(f"| {p['size'][1]}p / {p['path']} | " + " | ".join(cells) + " |")
summary.append("\n### Áudio e velocidade\n\n| Velocidade | Frequência medida (Hz), alvo 440 | Preserva tom no mixer |\n| --- | ---: | --- |")
for p in raw["pitch"]:
    summary.append(f"| {p['speed']}× | {p['measuredHz']:.2f} | {'sim' if p['pass'] else 'não'} |")
summary.append("\nO teste de tom mede a mistura offline, sem cpal/AAC. A sincronização audiovisual, os limites de clipe e a preservação do tom no arquivo final continuam no aceite de M3.3.\n")
summary.append("### Seleção proposta por item\n\nAs faixas de cada parâmetro estão no anexo JSON. `measured` indica paridade e mudança visível na configuração testada; `pending`, evidência insuficiente; `blocked`, divergência/ausência do caminho necessário; `conditional`, depende da integração.\n\n| Fase | ID público estável | Nome sugerido | Pasta | Evidência |\n| --- | --- | --- | --- | --- |")
for item in items:
    summary.append(f"| {item['phase']} | `{item['id']}` | {item['name']} | {' / '.join(item['folder'])} | {item['status']} |")
summary.append("\n### Casos com divergência\n")
failed = [c for c in raw["cases"] if not c["pass"]]
summary.append(", ".join(f"`{c['id']}`" for c in failed) + ".\n" if failed else "Nenhuma divergência acima da tolerância nos quadros comparados.\n")
summary.append("### Casos sem mudança visível\n")
summary.append(", ".join(f"`{c['id']}`" for c in raw["cases"] if c.get("observable") is False and c["kind"] != "baseline") + ".\n")
summary.append("Ficam pendentes de um padrão/parametrização específicos. Não usar essas passagens como prova de suporte.\n")

path = ROOT / "docs/m3.0-proposta.md"
text = path.read_text(encoding="utf-8")
start, end = "<!-- M3_RESULTS_START -->", "<!-- M3_RESULTS_END -->"
block = start + "\n" + "\n".join(summary) + "\n" + end + "\n\n"
if start in text:
    before, rest = text.split(start, 1)
    _, after = rest.split(end, 1)
    text = before + block + after.lstrip("\n")
else:
    text = text.replace("## Curadoria\n", block + "## Curadoria\n")
path.write_text(text, encoding="utf-8")
print(f"{len(raw['cases'])} casos; {sum(c['pass'] for c in raw['cases'])} com paridade; {len(items)} itens propostos.")
