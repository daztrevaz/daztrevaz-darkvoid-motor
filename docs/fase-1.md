# Fase 1 · Estúdio de legendas

Objetivo: legendar um vídeo dentro da aba Editor, do jeito do canal: transcrever, corrigir o texto
e os tempos, escolher o estilo (frase por frase, palavra por palavra, karaokê ou palavra que
salta), ver na prévia ao vivo e exportar com a legenda gravada no vídeo ou como `.srt`.

Aprovada por DTRZ em 15/11, depois do Portão 0. Contrato: `protocolo-motor.md`, seção 6 (versão 1).

## Decisões

- **Quem desenha a legenda é o motor**, na prévia e na exportação, para as duas ficarem iguais.
  O FilmCraft já tem trilhas de legenda, estilo, gravação no vídeo e `.srt`; falta a animação por
  palavra.
- **Fontes:** a Inter vem no motor; as outras são as instaladas no Windows (nada de fonte embutida
  nova no repositório).
- **As frases são montadas pela tela** a partir das palavras transcritas (pontuação, pausas,
  limite de caracteres e de duração). O motor recebe frases prontas.
- **Fora da Fase 1:** tradução de legendas, salvar o projeto em arquivo (Fase 2) e várias faixas
  de legenda ao mesmo tempo.

## Ordem e dependências

```
I1.1 (Claude: contrato v1 + simulado) ──► M1.1 (Codex: legendas animadas) ──┐
                                      └─► M1.2 (Codex: onda de áudio) ─────┤
D1.1 (Claude: frases e trilha) ──► D1.2 (Claude: estilos) ──► D1.3 ────────┴──► I1.2 ──► Portão 1
```
D1.x começa já, com o simulado desenhando as legendas no navegador.

## Tarefas

### I1.1 · Contrato v1 e simulado (Claude)
- Seção 6 do protocolo; tipos em `src/lib/motor/types.ts`; o simulado desenha as legendas nos
  4 modos e gera uma onda de áudio falsa.
- Aceite: os 4 modos aparecem na prévia do `npm run dev`.

### M1.1 · Legendas animadas no motor (Codex)
- `captions.set`, `captions.clear`, `captions.exportSrt`; `burnCaptions` no `export.start`;
  `hello` com `protocol: 1` e `fonts`.
- Os 4 modos da seção 6, com o texto do FilmCraft (`crates/text`, `crates/captions`). Mudanças nos
  crates do FilmCraft só onde precisar (ex.: estilo por palavra), pequenas e explicadas no PR.
- `captions.set` redesenha o quadro atual em menos de 50 ms (prévia ao vivo ao editar o estilo).
- Aceite: vídeo de 1 min exportado em cada modo com a legenda igual à prévia; `.srt` abre no
  YouTube Studio; play continua a 30 qps com legenda na prévia 1080p.

### M1.2 · Onda de áudio (Codex)
- `audio.peaks` com a duração inteira da sessão, calculado uma vez e guardado em cache em
  `<data-dir>\cache`.
- Aceite: 10 min de vídeo em menos de 3 s na primeira vez e instantâneo depois.

### D1.1 · Frases e trilha de legendas (Claude)
- Montar frases a partir das palavras (pontuação, pausa maior que 0,6 s, até 2 linhas / 42
  caracteres por linha no horizontal e menos no vertical, de 1 a 7 s).
- Trilha L1 com os blocos de frase: clicar seleciona e leva a reprodução; arrastar bordas ajusta
  o tempo; dividir (C) e juntar; editar o texto de uma frase sem perder o tempo das palavras.
- Lista de frases no painel Legendas, sincronizada com a reprodução; desfazer e refazer.
- Aceite: corrigir uma palavra errada e reajustar uma frase no vídeo da vela em menos de 1 min.

### D1.2 · Estilos (Claude)
- Painel de estilo: modo (os 4, com miniatura animada), fonte, peso, tamanho, cores, contorno,
  sombra, caixa, maiúsculas, posição (arrastando na prévia, com a área segura dos Shorts) e largura.
- Modelos prontos do canal (ex.: "DARKVOID clássico", "Karaokê bordô") e salvar os próprios.
- Aceite: trocar de estilo e ver na prévia na hora.

### D1.3 · Exportar com legenda (Claude)
- Na janela de exportação: "Gravar legendas no vídeo" e "Salvar também o .srt".

### I1.2 · Integração (Claude)
- Liga D1.x ao motor real (M1.1 e M1.2), atualiza o motor do instalador e publica a versão.

## Portão 1 (teste de DTRZ)

1. Transcrever o vídeo da vela, corrigir uma palavra e o tempo de uma frase.
2. Ver os 4 estilos na prévia, durante o play, sem perder fluidez.
3. Exportar o Short com legenda gravada (estilo à escolha) e o `.srt`; conferir no player e no
   YouTube Studio (rascunho privado).

Passou: Fase 2 (editor base: cortes, várias mídias e salvar projeto).
