# Protocolo do Motor DARKVOID · versão 1 (Fases 0 e 1)

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

## 7. Regras

- O motor só lê os caminhos que a tela manda e só grava no caminho de exportação pedido e dentro
  de `--data-dir`.
- Download de modelo só depois de a tela mostrar tamanho, origem e licença e a pessoa confirmar.
  Origem: Hugging Face (repositórios da OpenAI), conferido por SHA-256, como no FilmCraft.
- Nada de rede além do download de modelos. Sem telemetria.
- Tempos em segundos (`f64`) na fronteira; por dentro o motor usa os ticks exatos do FilmCraft.
