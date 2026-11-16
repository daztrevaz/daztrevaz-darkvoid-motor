# Fase 3 · Transições, cor, filtros, efeitos e upscale

Objetivo: dar acabamento ao vídeo dentro do editor, no nível de um editor profissional, com a
prévia igual ao arquivo exportado.

Aprovada por DTRZ em 16/11, depois do Portão 2. Plano completo: `voidappyt/docs/editor/fase-3.md`.
Dividida em duas entregas:

- **3a:** transições, cor e filtros.
- **3b:** efeitos, movimento com pontos-chave, velocidade do clipe e upscale.

## Decisões

- **Usamos o que o engine do FilmCraft já tem.** O catálogo de transições (`crates/project/src/vtransition.rs`),
  de efeitos (`crates/project/src/effect*.rs`), a cor (`crates/render`, `crates/engine/src/color.rs`,
  `lumetri_presets.rs`, LUTs) e os pontos-chave (`crates/project/src/keyframe.rs`). Nada é escrito do
  zero se o engine já faz.
- **Só entra o que funciona de ponta a ponta:** prévia na placa e exportação iguais, nos dois
  caminhos (placa e processador), com desempenho aceitável em projeto de 1 h. Por isso a fase começa
  com a medição M3.0.
- **O motor entrega o catálogo** (`catalog.get`): identificador estável, nome em português,
  pasta e parâmetros. A tela não guarda lista própria de 100 transições.
- **Nomes de marca:** o painel se chama "Cor" (não "Lumetri") e nenhum nome de marca de terceiros
  aparece na tela. Os nomes em português são nossos.
- **Fora:** vídeo imersivo/VR, transições de texto, multicâmera, máscaras desenhadas à mão (podem
  virar sugestão depois).
- **Upscale clássico** (redimensionamento de alta qualidade + nitidez leve, fiel, sem inventar
  detalhe). A exportação tem um campo "método" para que um upscale por IA local possa entrar no
  futuro **só se DTRZ pedir** (exigiria mudar a regra "sem IA no produto" nos dois repositórios).
- **Pontos-chave** com interpolação linear, suave (entrada/saída) e parada; o editor de curvas de
  velocidade completo fica de fora.

## Decisões depois da M3.0 (16/11)

- **50 transições na 3a**, tiradas das 74 aprovadas na M3.0 (todas com prévia igual à exportação).
  **Sem novas medições:** a M3.0 basta. Se alguma travar ou ficar pesada no uso, DTRZ pede e ela sai
  do catálogo.
- Ficam de fora: corte suave por análise (`morph_cut`), ferramentas inteligentes (`motion_camera`,
  `motion_tween`, `shape_flow`, `shape_dissolve`) e as demais não listadas.
- Rotação só entra quando a divergência for corrigida. Upscale clássico precisa de filtro novo (M3.4).
- Intensidade dos looks (0 a 100%) é trabalho da M3.2.
- Contrato: seção 8 de `protocolo-motor.md` (versão 3).

### As 50 transições

| Pasta | ID interno do engine → nome na tela |
| --- | --- |
| Dissoluções (9) | `cross_dissolve` Dissolução cruzada · `dip_to_black` Passagem pelo preto · `dip_to_white` Passagem pelo branco · `dip_to_color` Passagem por cor · `blur_dissolve` Dissolução com desfoque · `additive_dissolve` Dissolução aditiva · `film_dissolve` Dissolução de filme · `luma_fade` Dissolução por brilho · `mosaic_transition` Mosaico |
| Animação (6) | `flip_motion` Virar · `fold_motion` Dobrar · `pop_motion` Saltar · `spin_motion` Girar · `spring_motion` Mola · `travel_motion` Viagem |
| Luzes e desfoques (11) | `cross_zoom` Zoom cruzado · `zoom_blur` Desfoque de zoom · `radial_blur` Desfoque radial · `directional_blur_transition` Desfoque direcional · `lens_blur` Desfoque de lente · `flash` Clarão · `flare` Reflexo de lente · `glow` Brilho · `light_leak` Vazamento de luz · `light_sweep` Varredura de luz · `chroma_leak` Vazamento de cor |
| Distorções (8) | `glitch` Falha digital · `vhs_damage` Fita VHS · `tv_power` TV desligando · `earthquake` Terremoto · `flicker` Tremulação · `glass` Vidro · `kaleidoscope` Caleidoscópio · `liquid_distortion` Distorção líquida |
| Deslocamentos (6) | `push` Empurrar · `slide` Deslizar · `split` Dividir · `whip` Chicote · `roll` Rolar · `stretch` Esticar |
| Transformações (5) | `spin_3d` Giro 3D · `page_peel` Virar página · `louver` Persiana · `slice` Fatiar · `wave` Onda |
| Varreduras (5) | `linear_wipe` Varredura linear · `soft_wipe` Varredura suave · `radial_wipe` Varredura radial · `clock_wipe` Relógio · `star_wipe` Estrela |

IDs públicos: `transition.<id interno>`, exceto `mosaic_transition` → `transition.mosaic` e
`directional_blur_transition` → `transition.directional_blur`.

## Decisões para a 3b (16/11)

- **61 efeitos**, tirados dos 80 aprovados na M3.0 (sem novas medições, mesma regra da 3a: o que
  travar no uso sai do catálogo a pedido de DTRZ). Mais **recortar**, **26 modos de mesclagem** e
  **chroma key** (pedidos de DTRZ depois da lista de recursos do FilmCraft).
- Fora: `posterize` (reprovado na M3.0), conversões técnicas (`cineon_converter`, `asc_cdl`,
  `gamma_correction`, `proc_amp`, `video_limiter`), `metadata_burnin`, `spacer`, `simple_text`
  (títulos ficam na Fase 4), `auto_reframe`, `auto_align`, `clone`, `extract`, `grow`, `shrink`,
  `logo_cutout`, `move`, `transform`, `spin`, `reduce_interlace_flicker`, `offset` de campo e afins.
- Rotação entra quando a divergência placa × processador for corrigida (parte da M3.3).
- Contrato: `transform` ganha `blendMode` (os 26 modos) e `crop` (esquerda, topo, direita, base, em %),
  ambos anunciados no catálogo; resto da seção 8 sem mudança.

### Os 61 efeitos

| Pasta | ID interno → nome na tela |
| --- | --- |
| Desfoque e nitidez (8) | `gaussian_blur` Desfoque gaussiano · `directional_blur` Desfoque direcional · `bokeh_blur` Desfoque bokeh · `focus_blur` Desfoque de foco · `compound_blur` Desfoque composto · `channel_blur` Desfoque por canal · `sharpen` Nitidez · `unsharp_mask` Máscara de nitidez |
| Distorção (9) | `lens_distortion` Distorção de lente · `spherize` Esferizar · `twirl` Redemoinho · `wave_warp` Ondulação · `turbulent_displace` Deslocamento turbulento · `magnify` Lupa · `mirror` Espelho · `offset` Deslocar imagem · `corner_pin` Fixar cantos |
| Movimento e câmera (6) | `camera_shake` Tremor de câmera · `wiggle` Balanço · `strobe` Estroboscópio · `posterize_time` Redução de quadros · `basic_3d` 3D básico · `rotate_3d` Rotação 3D |
| Luzes e brilho (8) | `lens_flare` Reflexo de lente · `light_leaks` Vazamento de luz · `glint` Cintilar · `edge_glow` Brilho nas bordas · `wonder_glow` Brilho mágico · `volumetric_rays` Raios de luz · `lighting_effects` Iluminação · `echo_glow` Brilho com eco |
| Estilização (13) | `black_white` Preto e branco · `invert` Inverter cores · `find_edges` Contornos · `color_emboss` Relevo colorido · `mosaic` Mosaico · `noise` Ruído · `rgb_split` Separação RGB · `brush_strokes` Pinceladas · `roughen_edges` Bordas ásperas · `stroke` Contorno · `drop_shadow` Sombra projetada · `long_shadow` Sombra longa · `vignette` Vinheta |
| Cor (8) | `tint` Tingir · `color_pass` Manter uma cor · `color_replace` Trocar cor · `levels` Níveis · `channel_mix` Mixer de canais · `brightness_contrast` Brilho e contraste · `four_color_gradient` Gradiente de 4 cores · `gradient` Gradiente |
| Transformação (4) | `horizontal_flip` Inverter na horizontal · `vertical_flip` Inverter na vertical · `rounded_crop` Recorte arredondado · `warp_stabilizer` Estabilizador |
| Chaves e máscaras (5) | `ultra_key` Chroma key · `color_key` Chave de cor · `luma_key` Chave de brilho · `track_matte` Máscara por trilha · `alpha_adjust` Ajuste de transparência |

IDs públicos: `effect.<id interno>`.

## Ordem e dependências

```
M3.0 (Codex: medição e proposta) ──► I3.1 (Claude: contrato v3 + simulado)
   3a: M3.1 transições ─┐   D3.1 transições ─┐
       M3.2 cor/filtros ┴─► D3.2 cor/filtros ┴─► I3.2 (versão) ──► Portão 3a
   3b: M3.3 efeitos/movimento/velocidade ─┐   D3.3 efeitos e pontos-chave ─┐
       M3.4 upscale clássico ─────────────┴─► D3.4 upscale na exportação ──┴─► I3.3 ──► Portão 3b
```

## Tarefas

### M3.0 · Medição e proposta (Codex)
- Levantar, no `darkvoid-motor.exe`, quais transições, efeitos de vídeo, ajustes de cor, looks e LUTs
  funcionam de ponta a ponta (placa e processador iguais; prévia igual à exportação).
- Medir o custo em 1080p e 2160p na prévia e na exportação (RTX 3060 Ti como alvo; anotar a máquina
  usada) e o custo dos osciloscópios (forma de onda, vetorscópio).
- Entregar `docs/m3.0-proposta.md`: lista curada (o que entra, o que fica de fora e por quê), nomes em
  português sugeridos, parâmetros de cada item e um rascunho do contrato v3. **Não muda o contrato
  sozinho.**

### I3.1 · Contrato v3 e simulado (Claude)
- A partir da M3.0: `catalog.get`; no `Clip`, transição de entrada, cor, look com intensidade,
  efeitos com parâmetros e pontos-chave, transformação (posição, escala, rotação, opacidade) e
  velocidade; `export.start` com escala (`method: "classic"`, nitidez).
- Simulado com prévia aproximada de tudo, para a tela andar sem o motor.

### M3.1 · Transições (Codex)
- Transições da lista curada entre clipes vizinhos da mesma trilha, com duração e parâmetros;
  sobreposição dos dois clipes durante a transição; áudio com cruzamento suave.
- As 50 transições da lista acima, com `catalog.get`, nomes e parâmetros em português.
- Aceite: as 50 aplicáveis por `timeline.set` (teste automático de cada uma: aplica, renderiza,
  muda a imagem); projeto com 5 transições exporta sem erro. Sem refazer a medição de paridade da M3.0.

### M3.2 · Cor e filtros (Codex)
- Correção básica, criativo (looks com intensidade, filme desbotado, nitidez, vibração), curvas,
  rodas de cor, correção secundária por HSL, vinheta; LUT `.cube` importada pela pessoa.
- Looks prontos (os do engine) com nomes em português.
- Aceite: cada ajuste igual na prévia e na exportação; 1 h de projeto com cor em todos os clipes
  exporta sem erro; osciloscópios só se a M3.0 mostrar que cabem.

### D3.1 · Transições (Claude)
- Galeria com prévia animada, arrastar para o encontro de dois clipes, duração pelas bordas na
  linha do tempo, parâmetros em Propriedades, aplicar a todos os cortes selecionados.

### D3.2 · Cor e filtros (Claude)
- Painel "Cor" na ordem de trabalho (básico, criativo, curvas, rodas, HSL, vinheta), antes e depois,
  galeria de filtros com miniatura do quadro atual e intensidade, copiar e colar a cor entre clipes,
  importar LUT. Respeita o modo leve.

### I3.2 · Integração 3a (Claude)
- Motor novo fixado, versão do DarkFlow, publicação, passos do Portão 3a.

### M3.3 · Efeitos, movimento e velocidade (Codex)
- Os 61 efeitos acima com parâmetros e pontos-chave; transformação (posição, escala, rotação,
  opacidade, `blendMode` com os 26 modos, `crop`) com pontos-chave; velocidade do clipe (0,25× a 4×)
  com o áudio acompanhando sem mudar o tom; corrigir a divergência da rotação.
- Aceite: cada efeito aplicado por `timeline.set` muda a imagem (teste automático); zoom lento com
  pontos-chave, um desfoque animado e um chroma key exportam iguais à prévia. Sem refazer a M3.0.

### M3.4 · Upscale clássico (Codex)
- Ao exportar numa resolução maior que a do arquivo, escala de alta qualidade com nitidez ajustável
  (0 a 100%), na placa quando houver.
- Aceite: vídeo 1080p exportado em 2160p sem halos nem serrilhado visíveis; tempo de exportação
  medido.

### D3.3 · Efeitos e pontos-chave (Claude)
- Função "Efeitos" na barra; recortar e modo de mesclagem em Ajustes; galeria de efeitos, lista de efeitos do clipe (ligar, desligar, reordenar), pontos-chave na
  agulha (adicionar, remover, ir ao anterior e ao próximo), faixa de pontos-chave em Propriedades,
  velocidade do clipe.

### D3.4 · Upscale na exportação (Claude)
- Resolução de saída maior que a original com aviso, método "Clássico" e nitidez.

### I3.3 · Integração 3b (Claude)
- Motor novo fixado, versão do DarkFlow, publicação, passos do Portão 3b.

## Andamento
- M3.0, M3.1 (50 transições) e M3.2 (cor, 26 looks, LUT, osciloscópios) concluídas; motor 0.4.0.
- D3.1, D3.2 e I3.2 concluídas; DarkFlow 0.24.0.
- Osciloscópios: o motor já entrega (`scopes.get`); a tela ainda não mostra (sugestão para depois).

## Portão 3a (teste de DTRZ)
1. Projeto com 5 transições diferentes; conferir na prévia e no arquivo exportado.
2. Cor diferente em 3 clipes (um com look, um com curvas, um com LUT); copiar a cor para outro clipe.
3. Exportar e comparar no player.

## Portão 3b (teste de DTRZ)
1. Zoom lento com pontos-chave numa imagem e um desfoque animado num vídeo.
2. Um clipe a 0,5× e outro a 2×, com o áudio acompanhando.
3. Exportar um vídeo 1080p em 2160p com o upscale clássico e comparar no player.
