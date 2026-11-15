# Motor DARKVOID — M0.2

Servidor local do protocolo de [`docs/protocolo-motor.md`](../../docs/protocolo-motor.md).
Não modifica o contrato nem os crates importados do FilmCraft.

## Uso

```powershell
cargo build -p darkvoid-motor
.\target\debug\darkvoid-motor.exe --token <64-caracteres-hexadecimais> --data-dir <pasta>
```

O processo escreve somente a linha JSON `ready` em stdout e escuta uma porta livre
em `127.0.0.1`. A tela conecta em `ws://127.0.0.1:<porta>/?token=<token>`.
Uma nova conexão autenticada substitui a anterior. EOF em stdin ou `shutdown`
encerra o motor e interrompe o áudio. Diagnósticos ficam em stderr.

Comandos implementados: `hello`, `media.probe`, `session.open`, `preview.config`,
`transport.play`, `transport.pause`, `transport.seek` e `shutdown`.
Exportação e transcrição pertencem a M0.3/M0.4; retornam `unknown_command`.
`hello` informa a GPU usada na composição, `software` como codificador e
`speech: false`. Não anuncia NVENC sem implementação e detecção.

## Reprodução

- Importação, sequência, enquadramento, decodificação, composição e mixagem usam o engine.
- O dispositivo padrão do `cpal` fornece o relógio; a posição desconta a latência
  informada pelo dispositivo. Sem dispositivo disponível, play retorna `audio_unavailable`.
- A mixagem estéreo usa um pool de mídia independente, na taxa do dispositivo.
  O callback recebe samples por fila limitada, sem locks, alocações ou decodificação.
- Pause e seek descartam o stream e os samples pendentes. Seek durante play recria
  a saída na posição pedida; seek pausado também envia JPEG e `state`.
- O compositor GPU do engine tem fallback CPU. Três workers codificam JPEG;
  filas limitadas e épocas impedem acúmulo ou publicação de quadros de seeks antigos.
  JPEGs concluídos fora de ordem não fazem a imagem voltar no tempo.
- A imagem enviada tem cabeçalho `DVF1` de 24 bytes e respeita o tamanho máximo,
  sem ampliar a sequência. Limites de entrada: dimensões de 1 a 3840 e qualidade de 1 a 100.
- `state` acompanha o relógio até 30 vezes/s; `stats` informa composição/JPEG,
  planejamento/decodificação e descartes uma vez/s durante play.
- Falta de dados no callback é registrada como `audio_underrun` em stderr ao parar.
  Os testes físicos falham se detectarem esse diagnóstico.

O motor não inicia autosave, downloads, serviços de IA ou acesso a serviços externos.
Mídia precisa estar em arquivo local. A pasta `cache` é criada sob `--data-dir`.

## Verificação

```powershell
cargo fmt -p darkvoid-motor
cargo fmt -p darkvoid-motor -- --check
cargo clippy -p darkvoid-motor --no-deps -- -D warnings
cargo test -p darkvoid-motor
```

Os testes do processo verificam token, substituição da conexão, erros, EOF,
shutdown, probe, formatos, configuração e JPEG real após seek pausado.
Um teste unitário força o caminho CPU. A mídia versionada é sintética.
Os testes físicos são ignorados por padrão para o CI não exigir placa de som.

```powershell
cargo test -p darkvoid-motor --test protocol playback_audio_clock_pause_resume_seek_and_end -- --ignored --nocapture
$env:MOTOR_BENCH_MEDIA = '<MP4-sintetico-1920x1080-de-10-minutos>'
$env:MOTOR_BENCH_SECONDS = '599'
cargo test -p darkvoid-motor --test protocol benchmark_1080p_audio_and_seek -- --ignored --nocapture
```

A medição conta JPEGs recebidos pelo WebSocket, confere avanço do relógio,
verifica ausência de underruns e mede seek até o JPEG em 50%, 90% e 10% da mídia.
A tolerância de medição é 29,5 qps para vídeo nominal de 30 fps; cada seek deve
ficar abaixo de 300 ms. O teste não substitui escuta humana ou validação no DarkFlow.
