# Protocolo do Motor DARKVOID · versão 2 (Fases 0 a 2)

Contrato entre a aba Editor do DarkFlow e o Motor DARKVOID (`darkvoid-motor.exe`, processo
auxiliar instalado junto com o app). Mudanças só com aprovação (ver `AGENTS.md`).

## 1. Ciclo de vida

1. O núcleo do DarkFlow (Rust, Tauri) inicia o motor como programa auxiliar:
   `darkvoid-motor.exe --token <32 bytes aleatórios em hex> --data-dir <pasta>`
   - `--data-dir`: pasta de trabalho (padrão `D:\DARKVOID\Editor`; modelos em `<data-dir>\modelos`,
     cache em `<data-dir>\cache`).
2. O motor escuta **somente em `127.0.0.1`**, numa porta livre escolhida pelo sistema, e escreve
   UMA linha JSON na saída padrão:
   `{"event":"ready","port":53817,"version":"0.1.0"}`
3. O núcleo repassa `port` e `token` para a tela. A tela abre
   `ws://127.0.0.1:<port>/?token=<token>`. Sem o token certo, a conexão é recusada.
4. **Encerramento:** quando a entrada padrão do motor fecha (o DarkFlow saiu ou travou), o motor
   termina sozinho. Também existe o comando `shutdown`.
5. Uma conexão de tela por vez. Uma segunda conexão derruba a primeira.

## 2. Mensagens (WebSocket)

Mensagens de **texto** são JSON. Mensagens **binárias** são quadros da prévia.

Pedido (tela → motor):
```json
{"id": 7, "cmd": "transport.seek", "params": {"seconds": 12.5}}
```
Resposta (motor → tela), sempre com o mesmo `id`:
```json
{"id": 7, "ok": true, "result": {}}
{"id": 7, "ok": false, "error": {"code": "media_not_found", "message": "Arquivo não encontrado."}}
```
Evento (motor → tela, sem `id`):
```json
{"event": "state", "data": {"playing": true, "seconds": 12.53}}
```
`message` de erro sempre em português do Brasil. `code` em inglês, estável, para o código tratar.

## 3. Quadro da prévia (binário)

```
bytes 0..4   "DVF1" (ASCII)
bytes 4..8   largura (u32, little-endian)
bytes 8..12  altura (u32, little-endian)
bytes 12..16 reservado (0)
bytes 16..24 tempo do quadro em segundos (f64, little-endian)
bytes 24..   imagem JPEG
```
A tela desenha o último quadro recebido num `<canvas>`. Quadros atrasados podem ser descartados
pelo motor; o áudio nunca para por causa da imagem.

## 4. Comandos da Fase 0

| Comando | Parâmetros | Resultado |
| --- | --- | --- |
| `hello` | — | `{version, gpu: {name, backend} \| null, encoders: ["software","nvenc_h264","nvenc_hevc"], speech: boolean}` |
| `media.probe` | `{path}` | `{duration, width, height, fps, hasAudio, videoCodec, audioCodec}` |
| `session.open` | `{path, format: "source" \| "vertical" \| "horizontal"}` | `{width, height, fps, duration}` (sequência com um clipe; `vertical` = 1080×1920 com o clipe encaixado, `horizontal` = 1920×1080) |
| `preview.config` | `{maxWidth, maxHeight, quality}` (`quality` 1 a 100) | `{}` |
| `transport.play` / `transport.pause` | — | `{}` |
| `transport.seek` | `{seconds}` | `{}` (envia o quadro daquele tempo mesmo pausado) |
| `export.start` | `{path, preset: "shorts_1080x1920" \| "youtube_1080p" \| "youtube_2160p", encoder: "software" \| "nvenc_h264" \| "nvenc_hevc", burnCaptions?: boolean}` (v1: `burnCaptions` grava as legendas de `captions.set` no vídeo; ausente = `false`) | `{jobId}` |
| `speech.models` | — | `{models: [{id: "whisper-tiny" \| "whisper-base" \| "whisper-small", sizeBytes, installed, license}]}` |
| `speech.download` | `{model}` | `{jobId}` |
| `speech.transcribe` | `{model, language: "pt" \| "en" \| "auto"}` (usa a mídia da sessão) | `{jobId}` |
| `job.cancel` | `{jobId}` (`jobId` é um número inteiro) | `{}` |
| `shutdown` | — | `{}` |

## 5. Eventos da Fase 0

| Evento | Dados |
| --- | --- |
| `state` | `{playing, seconds}` (até 30 por segundo durante o play; 1 a cada seek) |
| `stats` | `{fps, droppedFrames, decodeMs, renderMs}` (1 por segundo durante o play) |
| `job.progress` | `{jobId, kind: "export" \| "download" \| "transcribe", fraction, etaSeconds \| null}` |
| `job.done` | `{jobId, kind, result}`; export: `{path, bytes}`; download: `{model}`; transcribe: `{language, words: [{text, start, end, confidence}]}` |
| `job.error` | `{jobId, kind, error: {code, message}}` |

## 6. Versão 1 (Fase 1): legendas e onda de áudio

`hello` passa a informar também `protocol: 1` e `fonts: string[]` (famílias disponíveis: a Inter
embutida e as fontes instaladas no Windows, em ordem alfabética). Motor sem `protocol` = versão 0.

| Comando | Parâmetros | Resultado |
| --- | --- | --- |
| `captions.set` | `{cues: Cue[], style: CaptionStyle}` | `{}`; substitui as legendas da sessão e redesenha o quadro atual (prévia ao vivo) |
| `captions.clear` | — | `{}` |
| `captions.exportSrt` | `{path}` | `{path}`; frases de `cues` em SubRip, UTF-8 |
| `audio.peaks` | `{buckets}` (1 a 20000) | `{peaks: number[]}`: pico de 0 a 1 de cada fatia igual da duração da sessão |

Tipos:
```ts
type CueWord = { text: string; start: number; end: number };
type Cue = { start: number; end: number; words: CueWord[] }; // a frase é o texto das palavras
type CaptionStyle = {
  mode: "phrase" | "word" | "karaoke" | "pop";
  font: string;              // família (de hello.fonts); desconhecida = Inter
  weight: number;            // 100 a 900; usa o estilo mais próximo da família
  size: number;              // pixels numa imagem de 1080 linhas (escala com a saída)
  color: string;             // "#RRGGBB"
  activeColor: string;       // palavra atual (word, karaoke, pop)
  outline: { width: number; color: string } | null;     // largura em pixels a 1080 linhas
  shadow: { blur: number; offsetY: number; color: string; opacity: number } | null;
  box: { color: string; opacity: number; padding: number; radius: number } | null;
  uppercase: boolean;
  position: { x: number; y: number };   // centro do bloco de texto, fração da imagem (0 a 1)
  maxWidth: number;          // largura máxima do bloco, fração da largura da imagem
};
```

Modos (desenhados pelo motor na prévia e na exportação, iguais nas duas):
- `phrase`: a frase inteira de `start` a `end`.
- `word`: só a palavra que está sendo dita, em `activeColor`, do `start` ao `end` dela (entre duas
  palavras, a anterior fica até a próxima começar).
- `karaoke`: a frase inteira; as palavras já ditas e a atual em `activeColor`, as seguintes em `color`.
- `pop`: a frase inteira; a palavra atual em `activeColor` e com escala animada: 1,00 → 1,22 em
  80 ms → 1,12 em mais 80 ms, mantendo 1,12 até o fim da palavra.

Quebra de linha automática dentro de `maxWidth`; o bloco nunca sai da imagem. Frases não se
sobrepõem (a tela garante). Erros novos: `captions_invalid` (dados fora do formato),
`no_session`.

## 7. Versão 2 (Fase 2): linha do tempo, várias mídias e legenda por frase

`hello.protocol` passa a ser `2`. Tudo da versão 1 continua valendo; `session.open` continua
existindo (equivale a uma linha do tempo com um clipe só).

### Mídias

`media.probe {path}` também aceita imagens (PNG, JPG, WEBP) e áudios (WAV, MP3, M4A, AAC, FLAC) e
passa a devolver `kind`:
- `video`: como antes.
- `image`: `duration: 0`, `fps: 0`, `hasAudio: false`, `videoCodec` = formato da imagem.
- `audio`: `width: 0`, `height: 0`, `fps: 0`, `videoCodec: null`.
Arquivo que não é nenhum dos três: erro `unsupported_media`.

### Linha do tempo

| Comando | Parâmetros | Resultado |
| --- | --- | --- |
| `timeline.set` | `{format: "vertical" \| "horizontal", fps?: number, clips: Clip[]}` (`fps` padrão 30) | `{width, height, fps, duration}` |

```ts
type Clip = {
  id: string;                  // estável entre envios (a tela gera)
  media: string;               // caminho do arquivo
  track: "V1" | "V2" | "A1" | "A2";
  start: number;               // onde começa na linha do tempo (s)
  in: number;                  // trecho do arquivo usado: de `in` ...
  out: number;                 // ... até `out` (s); imagem: in = 0 e out = duração desejada
  volume: number;              // 0 a 2 (1 = original); vídeo: o áudio dele; imagem: ignorado
  fadeIn: number;              // s, rampa de volume no começo (0 = sem)
  fadeOut: number;             // s, rampa de volume no fim (0 = sem)
  muted?: boolean;             // vídeo sem som
};
```
- Primeiro `timeline.set` cria a sessão; os seguintes **atualizam** a mesma sessão (o motor compara
  pelos `id` e muda só o que mudou). Mantém `preview.config` e a posição da agulha (limitada à nova
  duração) e redesenha o quadro atual. Meta: resposta em menos de 50 ms com 300 clipes.
- Trocar `format` ou `fps` refaz a sessão (como `session.open` com outro formato).
- `V1` e `V2` recebem vídeos e imagens; `A1` e `A2`, só áudios. O som de um vídeo vai junto com o
  clipe de vídeo. `V2` é desenhada por cima de `V1`. Cada mídia é encaixada no quadro (sem cortar,
  fundo preto), como em `session.open`.
- Onde não há clipe: preto e silêncio. `duration` = fim do último clipe.
- Erro `timeline_invalid` (mensagem diz qual clipe) se: clipes da mesma trilha se sobrepõem,
  `out <= in`, `in < 0`, `out` além da duração do arquivo (vídeo e áudio), trilha errada para o tipo
  de mídia, número fora do intervalo. Arquivo que sumiu: `media_not_found` com o caminho.

### Onda de áudio e miniaturas

| Comando | Parâmetros | Resultado |
| --- | --- | --- |
| `audio.peaks` | `{media?, buckets}` | com `media`: picos do arquivo inteiro; sem `media`: da linha do tempo (como na v1) |
| `media.thumbs` | `{media, count, height}` (`count` 1 a 200, `height` 32 a 240) | `{thumbs: [{time, jpeg}]}`: `count` quadros igualmente espaçados na duração do vídeo; `jpeg` em base64 |

### Valem para a linha do tempo

- `speech.transcribe` ouve a mistura da linha do tempo; as palavras vêm no tempo da linha do tempo.
- `export.start` exporta a linha do tempo inteira (de 0 a `duration`).
- `captions.set`: tempos no tempo da linha do tempo (como antes). Cada frase pode ter
  `override` opcional, que substitui só aqueles campos do estilo naquela frase:
  ```ts
  type Cue = { start: number; end: number; words: CueWord[];
               override?: { position?: { x: number; y: number }; size?: number; maxWidth?: number } };
  ```
  Prévia e exportação desenham o `override` igual.

### Cache

`<data-dir>\cache` (ondas e miniaturas) tem limite de 5 GB: ao passar, o motor apaga os arquivos
usados há mais tempo. Nunca apaga nada fora do cache.

Erros novos: `timeline_invalid`, `unsupported_media`.

## 8. Versão 3 (Fase 3): transições, cor, looks, efeitos, movimento, velocidade e escala

`hello.protocol` passa a ser `3`. Tudo da versão 2 continua valendo; campos novos ausentes = comportamento
da v2. Base: medição M3.0 (`docs/m3.0-proposta.md` no motor).

### Catálogo

| Comando | Parâmetros | Resultado |
| --- | --- | --- |
| `catalog.get` | `{}` | `{version: 3, revision: string, items: CatalogItem[]}` |

```ts
type ParamValue = number | boolean | string | number[] | number[][];
type ParamSpec = {
  id: string;                       // estável
  name: string;                     // português do Brasil
  type: "float" | "bool" | "choice" | "point" | "color" | "curve" | "wheel" | "lut";
  min: number | null; max: number | null;
  default: ParamValue | null;
  defaultFrom?: "frameCenter" | "sourceCenter";
  choices?: { id: string; name: string }[];
  animatable: boolean;
  unit?: string;                    // "%", "px", "°", "s"
  group?: string;                   // seção do painel (ex.: "Básico", "Curvas", "Rodas de cor")
};
type CatalogItem = {
  id: string;                       // estável, ex.: "transition.cross_dissolve", "look.night"
  name: string;                     // português do Brasil, sem marcas de terceiros
  folder: string[];                 // ex.: ["Transições", "Luzes e desfoques"]
  kind: "transition" | "color" | "look" | "effect" | "transform" | "speed" | "scale";
  phase: "3a" | "3b";
  params: ParamSpec[];
  processing: "gpu" | "cpu" | "mixed";
  defaultDuration?: number;         // transições (s)
};
```
- O motor só anuncia o que já funciona na versão instalada (itens da 3b só aparecem depois da 3b).
- A tela não guarda cópia de nomes nem parâmetros: monta galerias e painéis a partir do catálogo.
- `color.grade` é um item só (`kind: "color"`), com os parâmetros agrupados por `group`
  (Básico, Criativo, Curvas, Rodas de cor, HSL, Vinheta).

### Campos novos do `Clip`

```ts
type Keyframe = { time: number; value: ParamValue; interpolation: "linear" | "easeIn" | "easeOut" | "hold" };
type AnimatedParam = { value: ParamValue; keyframes?: Keyframe[] };
type Effect = { id: string; instanceId: string; enabled: boolean; params: Record<string, AnimatedParam> };

type Clip = ClipV2 & {
  transitionIn?: { id: string; duration: number; params?: Record<string, ParamValue> };
  color?: { enabled: boolean; params: Record<string, ParamValue> };
  look?: { id: string; intensity: number };            // intensidade 0 a 100 (padrão 100)
  effects?: Effect[];                                  // ordem = ordem de processamento
  transform?: {
    position?: AnimatedParam;                          // [x, y] em pixels do quadro; padrão: centro
    scale?: AnimatedParam;                             // %, padrão: encaixe da v2 (100)
    rotation?: AnimatedParam;                          // graus (só quando o catálogo anunciar)
    opacity?: AnimatedParam;                           // %, 0 a 100, padrão 100
    blendMode?: string;                                // id de um dos modos anunciados; padrão "normal"
    crop?: { left: AnimatedParam; top: AnimatedParam; right: AnimatedParam; bottom: AnimatedParam }; // % de cada borda
  };
  speed?: number;                                      // 0,25 a 4, padrão 1; sem reverso
};
```
- **Transição** (`transitionIn`): pertence ao encontro com o clipe anterior **na mesma trilha** e
  **encostado** nele (fim do anterior = início deste). Centrada no corte: metade antes, metade depois.
  Usa o trecho do arquivo além do `out` do anterior e antes do `in` deste; onde o arquivo não tem esse
  trecho (imagem, começo ou fim do arquivo), o motor **repete o quadro da borda**. Duração > 0 e no
  máximo a menor das duas durações de clipe. Áudio dos dois clipes cruza suave durante a transição.
  Sem clipe anterior encostado: `timeline_invalid`.
- **Ordem de processamento:** cor → look → efeitos (na ordem da lista) → transformação/opacidade →
  transição entre os dois resultados. Copiar e colar entre clipes dá o mesmo resultado.
- **Mesclagem e recorte:** os modos de `blendMode` e o recorte vêm no catálogo (item `transform.motion`,
  parâmetros `blendMode` tipo `choice` e `crop_*`). Recorte aplicado antes da posição e da escala.
- **Velocidade:** `in`/`out` continuam em tempo do arquivo; a duração do clipe na linha do tempo passa
  a ser `(out - in) / speed`. O som acompanha **sem mudar o tom**. Sobreposição é validada com a
  duração nova. Fades de volume valem no tempo da linha do tempo.
- **Pontos-chave:** `time` em segundos **do arquivo** (mesmo domínio de `in`/`out`), como o engine.
  Assim cortar, aparar e dividir um clipe não mexe nos pontos. Depois do último ponto vale o valor dele;
  antes do primeiro, o valor do primeiro. Sem `keyframes`, vale `value`.
- **LUT:** o parâmetro `lut` (tipo `"lut"`) recebe o **caminho do arquivo `.cube`** escolhido pela
  pessoa. O motor valida (até 16 MB, tamanho de 2 a 65 pontos, só números finitos), guarda em cache
  pelo conteúdo e reaproveita. Inválido: `lut_invalid`. Sumiu do disco: `media_not_found`.
- Validação: parâmetro desconhecido, tipo errado, fora da faixa, pontos-chave fora de ordem ou com
  tempo repetido, item que o catálogo não anuncia → `timeline_invalid` dizendo o clipe e o campo.

### Exportação

`export.start` aceita `scale?: {method: "classic", sharpness: number}` (nitidez 0 a 100, padrão 0),
usado quando a resolução de saída é maior que a do arquivo: redimensionamento de alta qualidade
(Lanczos ou equivalente) e nitidez leve depois da escala. Só vale quando o catálogo anunciar o item
`scale.classic`; antes disso, ignorado. Outros métodos (ex.: IA local) só entram se o responsável
humano pedir e as regras dos dois repositórios mudarem.

### Osciloscópios (opcionais)

| Comando | Parâmetros | Resultado |
| --- | --- | --- |
| `scopes.get` | `{kinds: ("waveform" \| "vectorscope")[]}` | `{waveform?: string, vectorscope?: string}` (PNG em base64, do quadro atual) |

A tela pede no máximo 10 vezes por segundo e nunca no modo leve.

Erros novos: `lut_invalid`.

## 9. Versão 4 (Fase 4): áudio, títulos e exportações extras

`hello.protocol` passa a ser `4`. Tudo das versões anteriores continua valendo; campos novos ausentes
= comportamento da v3. O catálogo (`catalog.get`) anuncia o que existe; a tela não envia o que ele
não anuncia. Este rascunho foi escrito pelo Claude a partir do engine; o Codex propõe ajustes no PR
da M4.1 se o engine pedir outra forma (sem mudar sozinho).

### Catálogo: tipos novos

`CatalogItem.kind` ganha `"audioEffect"`, `"duck"`, `"title"` e `"exportFormat"`.

- `audio.<id>`: efeitos de áudio por clipe (limpeza de voz, equalizador, dinâmica). Parâmetros e
  pontos-chave como os efeitos de vídeo (seção 8).
- `audio.duck`: parâmetros do ducking da trilha.
- `title.text`, `title.shape`: o que um clipe de título aceita (fontes em `choices`).
- `export.gif`, `export.png`, `export.wav`: formatos extras (seção "Exportação").

### Campos novos do `Clip`

```ts
type Clip = ClipV3 & {
  gain?: number;                     // dB, -24 a +24, antes do volume (normalizar usa este campo)
  audioEffects?: ClipEffect[];       // ids "audio.*"; ordem = ordem de processamento; trilhas A1/A2 e clipes de vídeo com som
  crossfadeIn?: number;              // s, cruzamento de potência constante com o clipe anterior encostado na mesma trilha de áudio
  title?: Title;                     // clipe de título (V1/V2); `media` = "title:<id estável>"; `in` = 0, `out` = duração
};

type Title = { layers: TitleLayer[] };
type TitleLayer = {
  type: "text" | "rect" | "ellipse";
  box: [number, number, number, number];      // x, y, largura, altura em pixels do quadro do projeto
  text?: string;                              // só "text"; quebra de linha com \n
  font?: string;                              // uma das fontes de `hello.fonts`
  size?: number;                              // px
  bold?: boolean; italic?: boolean;
  align?: "left" | "center" | "right";
  fill: [number, number, number, number];     // RGBA 0..1
  stroke?: { color: [number, number, number, number]; width: number };
  shadow?: { color: [number, number, number, number]; blur: number; offset: [number, number] };
  radius?: number;                            // cantos do retângulo
};
```
- Título usa `transform` (posição, escala, rotação, opacidade com pontos-chave), `effects` e
  transições como qualquer clipe visual.
- `crossfadeIn` só vale com clipe anterior encostado na mesma trilha; senão `timeline_invalid`.

### Trilhas (ducking)

`timeline.set` aceita `tracks?: Partial<Record<"A1" | "A2", TrackAudio>>`:

```ts
type TrackAudio = {
  duck?: {
    by: ("V1" | "V2" | "A1" | "A2")[];   // trilhas com voz que fazem esta abaixar
    reduce: number;                       // dB, 0 a 40 (padrão 12)
    sensitivity: number;                  // 0 a 100 (padrão 50)
    fade: number;                         // s, subida e descida (padrão 0,8)
  };
};
```
O motor detecta a voz nas trilhas `by` e abaixa esta trilha; prévia igual à exportação.

### Loudness

| Comando | Parâmetros | Resultado |
| --- | --- | --- |
| `audio.measure` | `{clipId?: string}` | `{jobId}`; job `loudness` com `{integrated, range, truePeak, maxMomentary, maxShortTerm}` (LUFS, LU, dBTP) da mistura ou de um clipe |

Evento `meter` durante a reprodução, até 10 por segundo: `{momentary, shortTerm, truePeakL, truePeakR}`.

`export.start` aceita `loudness?: {target: number, truePeak: number}` (padrão do canal: −14 LUFS,
−1 dBTP); ausente = o comportamento atual da exportação.

### Exportação

`export.start` aceita `format?: "mp4" | "gif" | "png" | "wav"` (padrão `"mp4"`):
- `gif`: `{fps (5 a 30), maxWidth}` em `gif?`; `png`: sequência numerada numa pasta (`path` é a pasta);
  `wav`: só o áudio da mistura, 48 kHz, 24 bits.

### Legendas

`captions.importSrt {path}` → `{cues: Cue[]}` (a tela coloca na trilha; erros `srt_invalid`).

Erros novos: `srt_invalid`.

## 10. Regras

- O motor só lê os caminhos que a tela manda e só grava no caminho de exportação pedido e dentro
  de `--data-dir`.
- Download de modelo só depois de a tela mostrar tamanho, origem e licença e a pessoa confirmar.
  Origem: Hugging Face (repositórios da OpenAI), conferido por SHA-256, como no FilmCraft.
- Nada de rede além do download de modelos. Sem telemetria.
- Tempos em segundos (`f64`) na fronteira; por dentro o motor usa os ticks exatos do FilmCraft.
