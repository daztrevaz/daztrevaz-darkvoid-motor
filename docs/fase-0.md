# Fase 0 · Fundação e prova de conceito

Objetivo: provar, no PC de edição (RTX 3060 Ti, Ryzen 5 5600X, 32 GB, disco D:), que a aba Editor
abre um vídeo, toca com som de forma fluida, exporta e transcreve. Plano completo: documento
"Editor DARKVOID e Legendas: plano de execução com custo zero".

## Equipe

| Quem | Papel na Fase 0 |
| --- | --- |
| **Claude (sessão principal)** | Coordena, escreve contratos e tarefas, prepara repositórios e CI, integra no DarkFlow (Tauri, cliente do motor), revisa e aprova PRs, publica versões |
| **Codex** | Escreve o motor em Rust no repositório `daztrevaz/daztrevaz-darkvoid-motor` (a maior parte do código) |
| **Claude Code** | Design e interface da aba Editor no DarkFlow (`src/editor/**`) |
| **DTRZ** | Cria repositórios e acessos, aprova cada PR importante e faz o teste do portão |

## Ordem e dependências

```
M0.1 (Claude) ──► M0.2 (Codex) ──► M0.3 (Codex) ──► M0.4 (Codex) ─┐
I0.1 (Claude: cliente + simulado) ──► D0.1 (Claude Code) ──► D0.2 ─┼──► I0.2 (Claude) ──► Portão 0
                                                                     ┘
```
D0.1 começa já, com o cliente simulado; não precisa esperar o motor.

## Tarefas

### M0.1 · Preparar o repositório do motor (Claude) · concluída em 15/11 (release motor-v0.0.1)
- Importar o FilmCraft no commit `59c6de0b9189c007ddd4eaa6c8b97e5a6c188937` para `daztrevaz/daztrevaz-darkvoid-motor`.
- Remover a interface deles (`apps/filmcraft`, `apps/filmcraft-web`, `crates/ui-egui`) e o servidor
  de IA do build padrão; manter `LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE` e créditos.
- Trocar o `AGENTS.md` e o `CLAUDE.md` deles pelas nossas regras.
- CI no Windows (executor padrão, gratuito em repositório público) que gera
  `darkvoid-motor-x86_64-pc-windows-msvc.exe` e publica numa release `motor-vX.Y.Z` com SHA-256.
- Aceite: build verde e release com o .exe e o checksum.

### M0.2 · Servidor e reprodução (Codex)
- Novo crate binário `crates/darkvoid-motor` com o protocolo de `docs/editor/protocolo-motor.md`
  (copiado para o repositório do motor): WebSocket em 127.0.0.1, token, linha `ready`, fim ao
  fechar a entrada padrão.
- Comandos `hello`, `media.probe`, `session.open`, `preview.config`, `transport.*`, `shutdown`.
- Reprodução com o **áudio como relógio** (cpal no dispositivo padrão do Windows); quadros
  renderizados pelo motor do FilmCraft (GPU quando houver), redimensionados para
  `preview.config` e enviados em JPEG; quadros atrasados descartados; eventos `state` e `stats`.
- Aceite: com um MP4 1080p de 10 min, play contínuo a pelo menos 30 qps na prévia 1080p no PC de
  edição, som sem cortes, seek em menos de 300 ms; testes automáticos do protocolo.

### M0.3 · Exportação (Codex)
- `export.start` com os 3 modelos e os 3 codificadores; NVENC quando disponível, com volta para o
  software; eventos de progresso; `job.cancel`.
- Aceite: 1 min exportado em cada modelo, abre no player do Windows e no YouTube Studio
  (rascunho), e `hello` informa os codificadores reais da máquina.

### M0.4 · Transcrição (Codex)
- Ativar o recurso `whisper` do FilmCraft; `speech.models`, `speech.download` (com SHA-256, para
  `<data-dir>\modelos`), `speech.transcribe` com tempo por palavra.
- Aceite: 1 min em PT-BR com o modelo small, palavras com início e fim, em menos de 1 min no
  Ryzen 5600X; download cancelável.

### I0.1 · Cliente do motor e simulado (Claude) · entregue em 15/11 (src/lib/motor)
- `src/lib/motor/`: tipos do protocolo, cliente WebSocket com pedidos e eventos tipados, leitor do
  quadro `DVF1`, e `mock.ts` (motor falso no navegador, gerando quadros e eventos).
- Aceite: o simulado toca um vídeo falso de 10 min a 30 qps no `npm run dev`.

### D0.1 · Design da aba Editor (Claude Code)
- Layout completo da aba Editor no visual DarkFlow, mesmo que a Fase 0 só use parte dele:
  monitor de prévia, barra de transporte, linha do tempo (trilhas de vídeo, áudio e legenda),
  painel de mídia, painel de propriedades, barra de status (fps, quedas, GPU, codificadores).
- Tudo ligado ao cliente simulado. Atalhos J, K, L e espaço.
- Aceite: tela navegável no `npm run dev`, aprovada por DTRZ (prints no PR).

### D0.2 · Tela de teste da Fase 0 (Claude Code)
- Dentro da aba Editor: abrir arquivo, escolher vertical ou horizontal, play/pause/seek, exportar
  (modelo e codificador), baixar modelo e transcrever, lista de palavras com tempo, painel de
  desempenho.
- Aceite: funciona inteira com o simulado.

### I0.2 · Ligar tudo no DarkFlow (Claude)
- Tauri: iniciar o motor como programa auxiliar com token, repassar porta e token, encerrar junto;
  CSP liberando só `ws://127.0.0.1`; CI do DarkFlow baixando o .exe da release do motor e
  conferindo o SHA-256; aba Editor no menu (visível só para o perfil dono, DTRZ).
- Aceite: versão do DarkFlow publicada pelo atualizador com a aba Editor funcionando com o motor real.

## Portão 0 (teste de DTRZ)

1. Abrir um vídeo de 1 min e um de 10 min do disco D: e dar play: imagem fluida em 1080p, som sem cortes.
2. Exportar o de 1 min como Short (NVENC) sem erro.
3. Transcrever o de 1 min em PT-BR e conferir as palavras.

Passou: Fase 1 (Estúdio de legendas). Não passou: plano B da prévia, antes da Fase 1.
