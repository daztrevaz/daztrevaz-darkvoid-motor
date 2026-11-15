//! Motor DARKVOID: processo auxiliar do editor do DarkFlow.
//!
//! Esqueleto da tarefa M0.1: só prova que o motor do FilmCraft compila no Windows e que o
//! executável é publicado. O servidor do protocolo (docs/protocolo-motor.md) é a tarefa M0.2.

use std::process::ExitCode;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|a| a == "--version") {
        println!("darkvoid-motor {VERSION}");
        return ExitCode::SUCCESS;
    }

    // Garante que o motor do FilmCraft está ligado ao binário (falha de build aparece no CI).
    let temp = filmcraft_engine::temp_dir();

    let info = serde_json::json!({
        "event": "error",
        "data": {
            "code": "not_implemented",
            "message": "O servidor do motor ainda não foi implementado (tarefa M0.2).",
            "version": VERSION,
            "temp": temp.display().to_string(),
        }
    });
    println!("{info}");
    ExitCode::FAILURE
}
