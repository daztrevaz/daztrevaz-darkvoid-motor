//! Narração por processo externo substituível. Sem Python ou pesos embutidos no motor.
use crate::protocol::{Result, error};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    io::{self, BufRead, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
};
use tokio::sync::broadcast;

const LIMIT: usize = 32;
const ROOT: &str = r"D:\DARKVOID\modelos-teste";

pub trait Backend: Send {
    fn catalogue(&self) -> Value;
    fn command(&self) -> io::Result<Command>;
}
pub struct QwenBase;
impl Backend for QwenBase {
    fn catalogue(&self) -> Value {
        let ready = Path::new(ROOT).join("venv/Scripts/python.exe").is_file()
            && Path::new(ROOT).join("models/qwen3-base/model.safetensors").is_file()
            && Path::new(ROOT).join("models/qwen3-base/speech_tokenizer/model.safetensors").is_file();
        json!({"provider":"qwen-base","available":ready,"voices":[
            {"id":"pt-c","name":"PT-BR (C)","language":"pt-BR","available":ready && Path::new(r"D:\DARKVOID\provavoz\prova2\referencias\voz-C.wav").is_file()},
            {"id":"en-en2","name":"EN (EN-2)","language":"en","available":ready && Path::new(r"D:\DARKVOID\provavoz\prova2\referencias-en\EN-2.wav").is_file()}
        ]})
    }
    fn command(&self) -> io::Result<Command> {
        let worker = Path::new(ROOT).join("workers/qwen_base.py");
        std::fs::create_dir_all(worker.parent().ok_or_else(|| io::Error::other("Pasta inválida"))?)?;
        std::fs::write(&worker, include_str!("../../../tools/n5.2/qwen_worker.py"))?;
        let mut command = Command::new(Path::new(ROOT).join("venv/Scripts/python.exe"));
        command.args(["-B", "-u"]).arg(worker);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000); // CREATE_NO_WINDOW; sem console auxiliar.
        }
        Ok(command)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Say {
    pub text: String,
    pub profile: String,
}
struct Pending {
    id: u64,
    request: Say,
}
struct Active {
    id: u64,
    child: Child,
    messages: mpsc::Receiver<Value>,
    output: PathBuf,
    result: Option<Value>,
    failure: Option<Value>,
    reader: std::thread::JoinHandle<()>,
}
pub struct Narration {
    backend: Box<dyn Backend>,
    directory: PathBuf,
    queue: VecDeque<Pending>,
    active: Option<Active>,
    ids: Arc<AtomicU64>,
    events: broadcast::Sender<Value>,
}
impl Narration {
    pub fn new(directory: PathBuf, ids: Arc<AtomicU64>, events: broadcast::Sender<Value>) -> Self {
        Self { backend: Box::new(QwenBase), directory, queue: VecDeque::new(), active: None, ids, events }
    }
    pub fn voices(&self) -> Value {
        self.backend.catalogue()
    }
    pub fn say(&mut self, request: Say) -> Result<Value> {
        if request.text.trim().is_empty() || request.text.chars().count() > 8000 {
            return Err(error("invalid_text", "Digite um trecho de até 8.000 caracteres."));
        }
        if self.queue.len() + usize::from(self.active.is_some()) >= LIMIT {
            return Err(error("narration_queue_full", "A fila de narração está cheia. Aguarde ou cancele um trecho."));
        }
        let available = self.backend.catalogue()["voices"]
            .as_array()
            .is_some_and(|voices| voices.iter().any(|voice| voice["id"] == request.profile && voice["available"] == true));
        if !available {
            return Err(error("voice_unavailable", "Perfil, modelo ou ambiente local de narração indisponível."));
        }
        let id = self.ids.fetch_add(1, Ordering::Relaxed);
        self.queue.push_back(Pending { id, request });
        Ok(json!({"jobId":id,"position":self.queue.len() + usize::from(self.active.is_some())}))
    }
    fn send(&self, value: Value) {
        let _ = self.events.send(value);
    }
    fn failed(&self, id: u64, message: &str) {
        self.send(json!({"event":"job.error","data":{"jobId":id,"kind":"narration","error":{"code":"narration_failed","message":message}}}));
    }
    pub fn cancel(&mut self, id: u64) -> bool {
        if let Some(index) = self.queue.iter().position(|job| job.id == id) {
            self.queue.remove(index);
        } else if self.active.as_ref().is_some_and(|job| job.id == id) {
            if let Some(mut job) = self.active.take() {
                let _ = job.child.kill();
                let _ = job.child.wait();
                let _ = job.reader.join();
            }
        } else {
            return false;
        }
        self.send(json!({"event":"job.error","data":{"jobId":id,"kind":"narration","error":{"code":"cancelled","message":"Narração cancelada."}}}));
        true
    }
    fn launch(&mut self, pending: Pending) -> io::Result<()> {
        std::fs::create_dir_all(&self.directory)?;
        // A new filename preserves previous versions and prevents overwrites after restart.
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(io::Error::other)?.as_nanos();
        let output = self.directory.join(format!("narracao-{stamp}-{}.wav", pending.id));
        let mut command = self.backend.command()?;
        let mut child = command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()?;
        let input = json!({"text":pending.request.text,"profile":pending.request.profile,"output":output,"seed":pending.id});
        let sent = child.stdin.take().ok_or_else(|| io::Error::other("Entrada indisponível"))?.write_all(format!("{input}\n").as_bytes());
        if let Err(error) = sent {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        let stdout = child.stdout.take().ok_or_else(|| io::Error::other("Saída indisponível"))?;
        let (send, messages) = mpsc::sync_channel(16);
        let reader = std::thread::spawn(move || {
            let mut reader = io::BufReader::new(stdout);
            loop {
                let mut line = String::new();
                let result = reader.by_ref().take(65537).read_line(&mut line);
                if !matches!(result, Ok(n) if n > 0 && n <= 65536) {
                    break;
                }
                if let Ok(value) = serde_json::from_str(&line) {
                    let _ = send.try_send(value);
                }
            }
        });
        self.active = Some(Active { id: pending.id, child, messages, output, result: None, failure: None, reader });
        Ok(())
    }
    pub fn poll(&mut self) {
        if let Some(job) = &mut self.active {
            while let Ok(value) = job.messages.try_recv() {
                if let Some(result) = value.get("result") {
                    job.result = Some(result.clone());
                }
                if let Some(failure) = value.get("error") {
                    job.failure = Some(failure.clone());
                }
                let fraction = value["fraction"].as_f64().unwrap_or(0.0).clamp(0.0, 1.0);
                let _ = self.events.send(
                    json!({"event":"job.progress","data":{"jobId":job.id,"kind":"narration","fraction":fraction,"etaSeconds":null,"stage":value["stage"]}}),
                );
            }
        }
        let finished = self.active.as_mut().and_then(|job| match job.child.try_wait() {
            Ok(Some(status)) => Some(status.success()),
            Ok(None) => None,
            Err(_) => Some(false),
        });
        if let Some(success) = finished {
            let Some(mut job) = self.active.take() else {
                return;
            };
            if !success {
                let _ = job.child.kill();
                let _ = job.child.wait();
            }
            let _ = job.reader.join();
            // Drain the final record after EOF; process exit can precede the reader thread.
            for value in job.messages.try_iter() {
                if let Some(result) = value.get("result") {
                    job.result = Some(result.clone());
                }
                if let Some(failure) = value.get("error") {
                    job.failure = Some(failure.clone());
                }
            }
            if success
                && job.failure.is_none()
                && job.result.as_ref().is_some_and(|value| {
                    value["path"].as_str().map(Path::new) == Some(job.output.as_path())
                        && value["duration"].as_f64().is_some_and(|seconds| seconds.is_finite() && seconds > 0.0)
                        && job.output.is_file()
                })
            {
                self.send(json!({"event":"job.done","data":{"jobId":job.id,"kind":"narration","result":job.result}}));
            } else {
                self.failed(job.id, "Não foi possível gerar a narração local. Confira o modelo e as referências.");
            }
        }
        if self.active.is_none()
            && let Some(job) = self.queue.pop_front()
        {
            let id = job.id;
            if self.launch(job).is_err() {
                self.failed(id, "Não foi possível iniciar o componente local de voz.");
            }
        }
    }
}
impl Drop for Narration {
    fn drop(&mut self) {
        if let Some(id) = self.active.as_ref().map(|job| job.id) {
            self.cancel(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Available;
    impl Backend for Available {
        fn catalogue(&self) -> Value {
            json!({"voices":[{"id":"fixed","available":true}]})
        }
        fn command(&self) -> io::Result<Command> {
            Err(io::Error::other("Sem processo nos testes de fila"))
        }
    }
    #[test]
    fn queue_is_bounded_cancellable_and_accepts_only_known_profiles() {
        let (events, mut rx) = broadcast::channel(64);
        let mut service = Narration::new(PathBuf::new(), Arc::new(AtomicU64::new(10)), events);
        service.backend = Box::new(Available);
        let say = || Say { text: "Texto de teste.".into(), profile: "fixed".into() };
        assert!(service.say(Say { text: " ".into(), profile: "fixed".into() }).is_err());
        assert!(service.say(Say { text: "Texto".into(), profile: "unknown".into() }).is_err());
        for _ in 0..LIMIT {
            service.say(say()).expect("vaga");
        }
        assert!(service.say(say()).is_err());
        assert!(service.cancel(10));
        assert_eq!(rx.try_recv().expect("cancelamento")["data"]["error"]["code"], "cancelled");
        assert!(!service.cancel(10));
        assert_eq!(service.say(say()).expect("vaga liberada")["jobId"], 42);
    }
}
