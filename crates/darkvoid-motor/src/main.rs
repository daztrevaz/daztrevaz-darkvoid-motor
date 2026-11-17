//! Processo auxiliar local do editor DarkFlow.
mod audio;
mod audio_effects;
mod cache;
mod captions;
mod catalog;
mod color;
mod effects;
mod encoding;
mod exports;
mod loudness;
mod mix;
mod motor;
mod peaks;
mod preview;
mod protocol;
mod server;
mod speech;
mod thumbs;
mod timeline;

#[cfg(test)]
mod audio_effects_tests;
#[cfg(test)]
mod audio_only_tests;

#[cfg(test)]
mod color_tests;
#[cfg(test)]
mod effects_tests;
#[cfg(test)]
mod measurement;
#[cfg(test)]
mod phase3_tests;
#[cfg(test)]
mod upscale_tests;

use std::{io::Read, path::PathBuf, process::ExitCode};
use tokio::sync::watch;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

struct Options {
    token: String,
    data_dir: PathBuf,
}

impl Options {
    fn parse(mut args: impl Iterator<Item = String>) -> Result<Self, &'static str> {
        let mut token = None;
        let mut data_dir = PathBuf::from(r"D:\DARKVOID\Editor");
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--token" if token.is_none() => token = args.next(),
                "--data-dir" => data_dir = args.next().map(PathBuf::from).ok_or("Informe a pasta de trabalho.")?,
                _ => return Err("Argumento inválido."),
            }
        }
        let token = token.filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())).ok_or("Informe --token com 32 bytes em hexadecimal.")?;
        Ok(Self { token, data_dir })
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--version"] {
        println!("darkvoid-motor {VERSION}");
        return ExitCode::SUCCESS;
    }
    let opts = match Options::parse(args.into_iter()) {
        Ok(opts) => opts,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::FAILURE;
        }
    };
    let (stop, stopped) = watch::channel(false);
    let eof = stop.clone();
    std::thread::spawn(move || {
        let mut buf = [0; 1024];
        while let Ok(n) = std::io::stdin().read(&mut buf) {
            if n == 0 {
                break;
            }
        }
        let _ = eof.send(true);
    });
    match server::run(opts.token, opts.data_dir, stop, stopped).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => {
            eprintln!("Não foi possível iniciar o servidor local.");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_token_and_arguments() {
        assert!(Options::parse(["--token".into(), "ab".repeat(32)].into_iter()).is_ok());
        for args in [vec![], vec!["--token", "abc"], vec!["--data-dir"], vec!["--unknown"]] {
            assert!(Options::parse(args.into_iter().map(str::to_owned)).is_err());
        }
    }
}
