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
- Aceite: 5 transições diferentes num projeto, prévia sem engasgar a 30 qps em 1080p, exportação
  igual à prévia (comparação de quadros), testes automáticos.

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
- Efeitos da lista curada com parâmetros e pontos-chave; transformação com pontos-chave; velocidade
  do clipe (0,25× a 4×) com o áudio acompanhando sem mudar o tom.
- Aceite: zoom lento com pontos-chave e um desfoque animado iguais na prévia e na exportação.

### M3.4 · Upscale clássico (Codex)
- Ao exportar numa resolução maior que a do arquivo, escala de alta qualidade com nitidez ajustável
  (0 a 100%), na placa quando houver.
- Aceite: vídeo 1080p exportado em 2160p sem halos nem serrilhado visíveis; tempo de exportação
  medido.

### D3.3 · Efeitos e pontos-chave (Claude)
- Galeria de efeitos, lista de efeitos do clipe (ligar, desligar, reordenar), pontos-chave na
  agulha (adicionar, remover, ir ao anterior e ao próximo), faixa de pontos-chave em Propriedades,
  velocidade do clipe.

### D3.4 · Upscale na exportação (Claude)
- Resolução de saída maior que a original com aviso, método "Clássico" e nitidez.

### I3.3 · Integração 3b (Claude)
- Motor novo fixado, versão do DarkFlow, publicação, passos do Portão 3b.

## Portão 3a (teste de DTRZ)
1. Projeto com 5 transições diferentes; conferir na prévia e no arquivo exportado.
2. Cor diferente em 3 clipes (um com look, um com curvas, um com LUT); copiar a cor para outro clipe.
3. Exportar e comparar no player.

## Portão 3b (teste de DTRZ)
1. Zoom lento com pontos-chave numa imagem e um desfoque animado num vídeo.
2. Um clipe a 0,5× e outro a 2×, com o áudio acompanhando.
3. Exportar um vídeo 1080p em 2160p com o upscale clássico e comparar no player.
