# Motor DARKVOID — Fase 0

Servidor local do protocolo de [`docs/protocolo-motor.md`](../../docs/protocolo-motor.md).
Contrato preservado. No código importado, o downloader de fala exige HTTPS,
inclusive em redirecionamentos; a inferência do FilmCraft não foi alterada.

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
`transport.play`, `transport.pause`, `transport.seek`, `export.start`, `job.cancel`,
`speech.models`, `speech.download`, `speech.transcribe` e `shutdown`.
`hello` informa a GPU usada na composição e `speech: true` (Whisper CPU disponível no build).
Os codificadores sempre incluem `software`; `nvenc_h264` e `nvenc_hevc` só entram
quando cada codec é inicializado com sucesso no driver, por meio da API segura existente.

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

## Exportação

`export.start` recebe `path`, `preset` e `encoder`, como no contrato, e devolve `jobId`.
Os três modelos reutilizam os presets do engine: Shorts 1080 × 1920, YouTube
1920 × 1080 e YouTube 3840 × 2160. Mantêm áudio AAC, frame rate da sessão e as
configurações de bitrate e enquadramento do preset. Shorts inclui normalização a −14 LUFS.

A exportação usa um snapshot da sessão inteira, compositor GPU com fallback CPU
e um pool de mídia independente. O worker tem limite de quatro threads do engine.
Uma exportação pode rodar por vez; outra tentativa retorna `export_busy`.

`software` seleciona H.264; os dois modos NVENC selecionam H.264 ou HEVC quando
disponíveis. Sem o codec solicitado, ou se o hardware falhar ao codificar, o job
usa H.264 por software. Uma falha durante a codificação reinicia o arquivo
temporário inteiro, sem misturar bitstreams. O fallback é registrado em stderr.

`job.progress` é emitido a cada 250 ms, com fração monotônica e ETA opcional;
`job.done` traz `{path, bytes}` depois de salvar o arquivo. Falhas e cancelamento
geram `job.error`, com código estável e mensagem em português. `job.cancel`
retorna imediatamente e aciona o cancelamento cooperativo. EOF e shutdown
cancelam o worker antes de encerrar o processo.

O destino deve ser um MP4 local em pasta existente. Arquivos existentes nunca
são sobrescritos. A codificação grava um temporário em `<data-dir>/cache`,
removido em caso de erro ou cancelamento. A publicação no mesmo disco é feita
sem sobrescrita. Entre discos, o motor copia para um arquivo novo no caminho
solicitado, verifica cancelamento e remove sua saída incompleta se a cópia falhar.

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
Os testes de exportação verificam as nove combinações de modelo/codificador,
dimensões, duração, áudio, progresso, erros, cancelamento e limpeza dos temporários.
Testes físicos e a medição de um minuto são ignorados por padrão no CI.

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

Para medir um minuto de exportação por modelo, com NVENC H.264 quando disponível:

```powershell
$env:MOTOR_EXPORT_MEDIA = '<MP4-sintetico-de-60-segundos>'
$env:MOTOR_EXPORT_DIR = '<pasta-existente-sem-os-arquivos-de-saida>'
cargo test --release -p darkvoid-motor --test protocol acceptance_export_one_minute_each_preset -- --ignored --nocapture
```

O teste mede do pedido até `job.done`, incluindo preparação, codificação e publicação.
Não usa FFmpeg no produto. FFmpeg pode ser usado fora do motor para gerar e conferir
as mídias sintéticas; o teste não comprova abertura no player nem upload ao YouTube Studio.

## Transcrição local — M0.4

O build ativa `whisper` e `speech-download` do engine e usa o reconhecedor Whisper
CPU do FilmCraft, com alinhamento por atenção e ajuste dos limites por atividade de voz.
O áudio é decodificado em blocos de um segundo, convertido para mono a 16 kHz e
processado no computador. Não há telemetria nem upload de áudio.

`speech.models` lista tiny, base e small, tamanho total em bytes, licença e instalação.
`installed: true` exige todos os artefatos com tamanho e SHA-256 corretos.
Os arquivos ficam em `<data-dir>/modelos/<id>/`; pesos nunca são versionados.

A tela deve mostrar tamanho, origem Hugging Face/OpenAI e licenças MIT/Apache-2.0
antes de confirmar. Somente depois envia `speech.download {model}`, como definido
no contrato existente. O pedido autenticado é a fronteira de confirmação do servidor;
não foi acrescentado um campo novo ao protocolo. `hello`, listagem e transcrição
nunca iniciam download. O catálogo fixa URLs HTTPS por revisão e SHA-256; não aceita URL do cliente.
O downloader do engine verifica os arquivos antes de publicá-los. Arquivos `.part`
são removidos ao terminar, cancelar ou falhar; arquivos completos verificados são preservados.

`speech.transcribe {model, language}` aceita `pt`, `en` ou `auto`. Usa a mídia da
sessão, independente do playhead, e exige o modelo instalado. O job faz uma nova
verificação SHA-256 antes da carga do modelo. A resposta final tem idioma e palavras
com `text`, `start`, `end` e `confidence`, com tempos em segundos. Limites inválidos
são ajustados à duração do áudio, palavras vazias ou sem duração são descartadas e
intervalos não retrocedem. A confiança é a fornecida pelo engine, que atualmente
usa 1 como padrão; não representa uma probabilidade calibrada de acerto.

Downloads e transcrições compartilham um worker de fala por processo. Uma nova
tentativa durante processamento retorna `speech_busy`; exportação mantém seu worker
independente. Todos os tipos de job usam o mesmo alocador de IDs para evitar colisões.
São emitidos `job.progress`, `job.done` e `job.error`, com mensagens em português.
`job.cancel` também cancela jobs de fala. EOF e shutdown sinalizam cancelamento e
aguardam o worker. O cancelamento é cooperativo: download verifica entre blocos
recebidos e Whisper verifica entre janelas; não interrompe um cálculo interno em curso.

Os testes padrão não acessam rede nem exigem modelos. A medição física exige
confirmação explícita e entrada sintética PT-BR de 60 segundos:

```powershell
$env:MOTOR_SPEECH_DOWNLOAD_CONFIRMED = '1'
$env:MOTOR_SPEECH_DATA_DIR = '<pasta-de-teste>'
$env:MOTOR_SPEECH_MEDIA = '<video-com-fala-pt-br-de-60-segundos>'
cargo test --release -p darkvoid-motor --test protocol acceptance_whisper_small_portuguese_one_minute -- --ignored --nocapture
```

O tempo de transcrição inclui a validação do modelo, sua carga, a decodificação de
áudio e a inferência. O download é medido separadamente. A meta de aceite é menos
de 60 segundos com small no Ryzen 5600X; aprovação depende da medição real.
