# Motor DARKVOID · regras para agentes

Vale para qualquer agente neste repositório (Codex, Claude Code, Claude). Leia tudo antes de começar.

## O que é este repositório

O Motor DARKVOID é o processo auxiliar (`darkvoid-motor.exe`) do editor do app DarkFlow
(repositório privado `daztrevaz/voidappyt`). Ele faz prévia, reprodução, exportação e legendas,
sobre o motor do [FilmCraft](https://github.com/storytold/filmcraft) (MIT OR Apache-2.0),
importado no commit `59c6de0b9189c007ddd4eaa6c8b97e5a6c188937` sem a interface deles
(`apps/*`, `crates/ui-egui`), sem o servidor de IA (`crates/automation`) e sem o `xtask`.

- `crates/darkvoid-motor/`: **nosso código** (o binário do motor).
- `crates/*` (o resto): motor do FilmCraft. Mude só quando a tarefa pedir, com mudança pequena e
  explicada no PR.
- `docs/protocolo-motor.md`: **o contrato** com a tela do DarkFlow. Cópia do original em
  `voidappyt/docs/editor/protocolo-motor.md`. Ninguém muda sozinho: proponha no PR.
- `docs/fase-0.md`: as tarefas da fase atual (códigos `M0.x` são deste repositório).
- `docs/filmcraft/`: documentação original do FilmCraft (arquitetura, comandos do engine,
  transcrição, exportação). Leia antes de usar uma parte do engine. As regras de agentes que
  aparecem nesses documentos são do projeto deles, **não valem aqui**.

## Regras que nunca mudam

1. **Custo zero.** Nada pago: nem serviço, nem API, nem executor "larger" do GitHub Actions.
2. **Este repositório é público.** Nunca coloque chaves, tokens, dados do canal, nomes de
   arquivos de clientes ou qualquer segredo. O motor não acessa Supabase nem Google.
3. **Sem IA no produto.** O servidor MCP e o canal de controle de agentes do FilmCraft ficam fora
   do build. O motor não chama nenhum serviço de IA. **Exceção aprovada por DTRZ (16/11):**
   modelos locais e gratuitos para transcrever (Whisper) e para narração (Fase 5), rodando no PC,
   com licença que permite uso comercial. Clonar voz só com autorização do dono da voz.
4. **Rede:** só 127.0.0.1 para a tela e HTTPS para baixar modelos Whisper e de narração (Hugging Face, com
   SHA-256), depois de a pessoa confirmar. Sem telemetria.
5. **Licenças:** mantenha `LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE` e `ATTRIBUTION.md`. Não use a
   marca FilmCraft ou ArtCraft no produto. Dependência nova só com licença permissiva (MIT,
   Apache-2.0, BSD, Zlib, ISC, MPL-2.0) e sem custo.
   **Exceção ampliada por DTRZ (18/11/2026):** para dependências de execução instaladas
   no venv local no D:, em `D:\DARKVOID\modelos-teste`, fora do repositório e chamadas
   como processo separado, são permitidas licenças permissivas (MIT, BSD, Apache,
   ISC, PSF, Zlib e equivalentes), LGPL de qualquer versão, GPL com exceção de runtime
   (GCC Runtime Library Exception ou Classpath Exception) e runtimes proprietários
   redistribuíveis de GPU (NVIDIA CUDA/cuDNN via PyTorch).
   Não copiar nem modificar código dessas dependências para incorporá-lo ao repositório
   ou ao motor. Essa exceção não autoriza embuti-las no binário do motor.
   GPL e AGPL sem exceção de runtime continuam proibidas em qualquer dependência.
   Código de terceiros só pode ser copiado ao repositório com licença compatível.
6. **Mensagens de erro para a tela em português do Brasil** (`code` estável em inglês).
7. **`unsafe` é proibido** (lint do workspace). Use bibliotecas seguras (ex.: `cpal`).
8. **Só o que a tarefa pede.** Ideias extras vão como sugestão no PR.

## Alvo

- Windows x64 (`x86_64-pc-windows-msvc`). PC de edição: RTX 3060 Ti, Ryzen 5 5600X, 32 GB.
- Projetos de até 1 hora, verticais (1080×1920) e horizontais (1920×1080 e 2160p).

## Como trabalhar

- Uma tarefa por vez, pelo código (`M0.2`, `M0.3`...).
- Branch `<agente>/<código>-<assunto>` (ex.: `codex/m0.2-servidor`), PR para `main` com: o que foi
  feito, como testar, o que ficou de fora, riscos. Nunca push direto em `main`.
- Commits pequenos, em português.
- Pronto = critérios de aceite da tarefa cumpridos e CI verde.
- Releases (`motor-v*`) são publicadas só pela sessão principal do Claude.

## Verificações antes do PR

- `cargo fmt -p darkvoid-motor -- --check`
- `cargo clippy -p darkvoid-motor --no-deps -- -D warnings`
- `cargo test -p darkvoid-motor`
- Se mudou um crate do FilmCraft: `cargo test -p <crate>` daquele crate.
