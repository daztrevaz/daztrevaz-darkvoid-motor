# Fase 2 · Editor base: linha do tempo, várias mídias e legenda por frase

Objetivo: editar de verdade. Cortar o vídeo longo, juntar vários clipes (vídeos, imagens, músicas)
e dar à legenda posição e tamanho por frase.

Aprovada por DTRZ em 16/11, depois do Portão 1. Contrato: `protocolo-motor.md`, seção 7
(versão 2). Plano completo (com as tarefas da tela): `voidappyt/docs/editor/fase-2.md`.

## Decisões

- **A linha do tempo é da tela.** A tela guarda clipes, cortes, desfazer e refazer, e envia a linha
  do tempo inteira em `timeline.set` a cada mudança. O motor monta a sequência no engine do
  FilmCraft (comandos `file.*`/edição dos `crates/edit` e `crates/project`), atualizando só o que
  mudou, e desenha, toca e exporta.
- **O projeto é salvo pela tela** (arquivo `.dvproj` do DarkFlow). O motor não grava projeto.
- **Fora da Fase 2:** transições, efeitos, cor, filtros e upscale (Fase 3).

## Tarefas do motor

### M2.1 · Linha do tempo no motor (Codex)
- `timeline.set` (seção 7): cria a sessão no primeiro envio e atualiza nos seguintes, comparando
  pelos `id`; mantém `preview.config` e a agulha; redesenha o quadro atual.
- `V1`/`V2` com vídeos e imagens (V2 por cima), encaixe no quadro como em `session.open`; vazios
  em preto. `media.probe` com `kind` e suporte a imagens e áudios.
- `export.start` e as legendas (`captions.set`, `burnCaptions`) valendo para a linha do tempo.
- Validação completa com `timeline_invalid` e `unsupported_media` (mensagens em português).
- Aceite: projeto de 1 h com 300 clipes do mesmo arquivo; `timeline.set` responde em menos de
  50 ms; play atravessa os cortes sem engasgar a 30 qps na prévia 1080p; seek em menos de 300 ms;
  exportação igual à prévia. Testes automáticos de validação, vazios, imagem parada e V2 sobre V1.

### M2.2 · Áudio por clipe e transcrição da linha do tempo (Codex)
- Som dos vídeos e áudios de `A1`/`A2` misturados; `volume` (0 a 2), `fadeIn`, `fadeOut`, `muted`.
  Sem estalo nos cortes.
- `audio.peaks {media}` por arquivo; sem `media`, da linha do tempo.
- `speech.transcribe` ouve a mistura da linha do tempo; palavras no tempo da linha do tempo.
- Aceite: música em A1 com volume 0,3 e fades por baixo de 3 vídeos, igual na prévia e no arquivo
  exportado; transcrição de uma linha do tempo com cortes devolve palavras nos tempos certos.

### M2.3 · Miniaturas, legenda por frase e cache (Codex)
- `media.thumbs` com cache em disco.
- `override` por frase (`position`, `size`, `maxWidth`) na prévia e na exportação.
- Cache de `<data-dir>\cache` com limite de 5 GB, apagando os arquivos usados há mais tempo.
- Aceite: 100 miniaturas de um vídeo de 10 min em menos de 3 s na primeira vez e instantâneas
  depois; uma frase com `override` sai igual na prévia e no vídeo exportado; cache acima do limite
  é reduzido sem tocar em nada fora dele.

## Ordem

M2.1 primeiro (as outras dependem da sessão de linha do tempo). Depois M2.2 e M2.3, em qualquer
ordem. Um PR por tarefa, branch `codex/m2.x-assunto`, como nas fases anteriores.

## Portão 2 (teste de DTRZ, com a tela pronta)

1. Vídeo de 10 min com 20 cortes e trechos apagados com recuo; legendas acompanham.
2. Projeto com 3 vídeos, 1 imagem e 1 música de fundo com volume baixo e fade.
3. Posição e tamanho de uma frase só, mudados direto na prévia.
4. Fechar sem salvar e reabrir: o projeto volta igual.
5. Exportar os dois projetos e conferir no player.
