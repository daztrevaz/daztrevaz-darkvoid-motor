# Fase 5 · Narração no editor

Objetivo: gerar as narrações dentro do editor, sem ElevenLabs e sem gastar créditos, com modelos
locais e gratuitos rodando na RTX 3060 Ti. Exceção à regra "sem IA" aprovada por DTRZ em 16/11
(ver AGENTS.md, regra 4).

## Decisões

- Candidatos (uso comercial permitido): **Qwen3-TTS** (Apache-2.0; voz criada por descrição e
  clonagem de 3 s; português) e **Chatterbox Multilingual v3** (MIT; modelo dedicado ao pt-BR;
  controle de intensidade; marca d'água inaudível). **Kokoro** (Apache-2.0) como reserva leve.
- Fora: XTTS-v2 e F5-TTS (licenças não comerciais).
- **Nada de clonar a voz da biblioteca da ElevenLabs** (voz de uma pessoa real, sem autorização).
  A voz do canal é criada por descrição ou clonada de alguém que autorize.
- Possível trabalho em equipe: o Qwen3-TTS cria a voz; o Chatterbox narra com essa voz como
  referência; o Whisper confere o áudio contra o roteiro e marca ou refaz o parágrafo com erro.
  Fica com os dois só se a prova mostrar ganho claro; senão, fica o melhor.

## Tarefas

### N5.0 · Prova de vozes (Codex)
- Gerar o mesmo roteiro de teste (cerca de 1 min, trecho real do canal, com pergunta, números e
  tensão) com: Chatterbox pt-BR; Qwen3-TTS com voz criada por descrição ("masculina, grave,
  imponente, pausada, narrador de documentário"); e a combinação (voz do Qwen3-TTS narrada pelo
  Chatterbox). Referência para comparar: a narração atual da ElevenLabs, fornecida por DTRZ.
- Medir: tempo de geração por minuto de áudio na RTX 3060 Ti, memória de vídeo, tamanho do download.
- Avaliar se roda no motor em Rust (ONNX/`ort` ou outra forma permissiva e gratuita) e o custo de
  cada caminho.
- Entregar os áudios (fora do Git), `docs/n5.0-prova.md` com os números e a recomendação. DTRZ ouve e escolhe.

### M5.1 · Narração no motor (Codex)
- Modelo escolhido baixado com confirmação e SHA-256 (como o Whisper); `speech.say` gera por
  parágrafo como job com progresso; vozes salvas no projeto; conferência pelo Whisper.

### D5.1 · Narração na tela (Claude)
- Função "Narração" na barra: roteiro colado, divisão por parágrafo, voz, velocidade, intensidade,
  pausas; cada parágrafo vira um clipe em A1; "gerar de novo" por parágrafo; marcação dos trechos
  que o Whisper apontou; legenda automática disponível.

### I5.1 · Integração (Claude)

## Portão 5 (teste de DTRZ)
1. Narrar um roteiro completo de um vídeo do canal e comparar com a ElevenLabs.
2. Refazer 2 parágrafos, ajustar pausas, gerar legendas e exportar.
