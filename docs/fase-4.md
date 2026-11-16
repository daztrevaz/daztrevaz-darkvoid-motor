# Fase 4 · Áudio, títulos e acabamento

Objetivo: deixar o vídeo pronto para publicar sem sair do editor: som no nível certo e limpo,
títulos, organização e segurança contra perda de trabalho. Itens escolhidos por DTRZ em 16/11 a
partir da lista de recursos do FilmCraft que ficaram de fora.

Começa depois do Portão 3b. A barra de menus e os atalhos (D4.0) começam antes, só na tela.

## Tarefas

### D4.0 · Menus e atalhos (Claude, já pode começar)
- Barra de menus do editor no estilo da marca: Arquivo, Editar, Clipe, Linha do tempo, Marcadores,
  Títulos, Exibir, Ajuda. Só itens que existem; cada um mostra o atalho; itens indisponíveis apagados.
- Tabela única de atalhos no padrão Premiere Pro adaptado para Windows (Cmd → Ctrl), só para o que
  o editor faz (base: `crates/engine/src/shortcut_presets.rs` do motor e `docs/filmcraft/keyboard.md`).
  As teclas 1 a 6 da barra de funções continuam.
- Ajuda ▸ Atalhos do teclado (F1 ou ?): janela com busca, por área, gerada da mesma tabela.
- Aceite: todo atalho da janela funciona; nenhum atalho repetido no mesmo lugar (teste automático
  da tabela).

### M4.1 · Loudness e normalização (Codex)
- Medidor de loudness (momentâneo, curto, integrado, pico real; BS.1770/EBU R128) por `audio.loudness`
  da linha do tempo; normalizar um clipe ou a mistura a um alvo (ex.: −14 LUFS, pico −1 dBTP).
- Aceite: leitura integrada igual à do engine nos testes dele; exportação normalizada mede o alvo.

### M4.2 · Limpeza de voz e efeitos de áudio (Codex)
- Por clipe: redução de ruído, removedor de chiado elétrico, equalizador, passa-altas e passa-baixas,
  compressor e limitador (os do FilmCraft), com parâmetros no catálogo.
- Ducking: a trilha de música abaixa sozinha quando a trilha de voz tem som (quanto, ataque e soltura).
- Cruzamento suave de áudio entre clipes encostados.
- Aceite: prévia igual à exportação; ducking audível e medido.

### M4.3 · Títulos e formas (Codex)
- Clipes de título numa trilha de vídeo: texto, retângulo, elipse; preenchimento, contorno, sombra,
  fundo; posição, escala, rotação e opacidade com pontos-chave; fontes do sistema.
- Aceite: título animado igual na prévia e na exportação, em vertical e horizontal.

### M4.4 · Exportações extras (Codex)
- GIF animado, sequência PNG e só o áudio (WAV).

### D4.1 · Áudio na tela (Claude)
- Medidor de loudness ao lado da prévia; Normalizar no menu Clipe; aba de efeitos de áudio em
  Ajustes; ducking por trilha; cruzamento ao arrastar o corte.

### D4.2 · Títulos na tela (Claude)
- Função "Títulos" na barra: modelos simples, digitar direto na prévia, alças para mover e girar,
  propriedades e pontos-chave.

### D4.3 · Acabamento (Claude)
- Marcadores coloridos com nome e duração (M), lista de marcadores.
- Copiar, recortar e colar clipes; colar inserindo.
- Recuperação após queda: diário gravado cerca de 1 s depois de cada edição e versões anteriores do
  projeto; ao reabrir depois de uma queda, oferecer recuperar.
- Osciloscópios (forma de onda e vetorscópio) na função Cor, até 10 por segundo, nunca no modo leve.
- Importar SRT para a trilha de legendas.

### I4.1 · Integração (Claude)
- Motor novo fixado, versão do DarkFlow, publicação, passos do Portão 4.

## Portão 4 (teste de DTRZ)
1. Vídeo com voz e música: limpar a voz, ducking na música, normalizar a −14 LUFS e conferir no medidor.
2. Título animado de abertura e uma tarja de nome.
3. Marcadores, copiar e colar clipes; fechar o app à força no meio da edição e recuperar.
4. Exportar MP4, um GIF curto e só o áudio.
