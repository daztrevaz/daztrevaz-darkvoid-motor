# Motor DARKVOID

Processo auxiliar do editor do **DarkFlow** (app interno do canal DARKVOID): prévia,
reprodução com áudio, exportação (incluindo NVENC) e legendas automáticas (Whisper local).

Construído sobre o motor do [FilmCraft](https://github.com/storytold/filmcraft), da equipe
ArtCraft, sob a licença MIT OR Apache-2.0, a partir do commit
`59c6de0b9189c007ddd4eaa6c8b97e5a6c188937`. A interface original, o servidor de controle por IA e
as ferramentas de build deles foram removidos. A documentação original está em `docs/filmcraft/`.

- Protocolo com a tela: [`docs/protocolo-motor.md`](docs/protocolo-motor.md)
- Tarefas da fase atual: [`docs/fase-0.md`](docs/fase-0.md)
- Regras para quem contribui (pessoas e agentes): [`AGENTS.md`](AGENTS.md)

## Build

```sh
cargo build --release -p darkvoid-motor
```

Releases `motor-v*` publicam `darkvoid-motor-x86_64-pc-windows-msvc.exe` e o SHA-256.

## Licença

MIT OR Apache-2.0, como o projeto original. Veja `LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE` e
`ATTRIBUTION.md`.
