//! Testes do processo real, sem rede externa e sem depender de áudio físico por padrão.
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use tokio::{net::TcpStream, time::timeout};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;
const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

struct Motor {
    child: Child,
    port: u16,
    _dir: tempfile::TempDir,
    stdout: BufReader<std::process::ChildStdout>,
    stderr: std::path::PathBuf,
}
impl Motor {
    fn start() -> Self {
        Self::start_with_data_dir(None)
    }
    fn start_with_data_dir(data_dir: Option<&std::path::Path>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let stderr = dir.path().join("stderr.log");
        let mut child = Command::new(env!("CARGO_BIN_EXE_darkvoid-motor"))
            .args(["--token", TOKEN, "--data-dir"])
            .arg(data_dir.unwrap_or(dir.path()))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(std::fs::File::create(&stderr).unwrap())
            .spawn()
            .unwrap();
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        let ready: Value = serde_json::from_str(&line).expect("linha ready JSON");
        assert_eq!(ready["event"], "ready");
        Self { child, port: ready["port"].as_u64().unwrap() as u16, _dir: dir, stdout, stderr }
    }
    fn url(&self, token: &str) -> String {
        format!("ws://127.0.0.1:{}/?token={token}", self.port)
    }
    async fn connect(&self) -> Socket {
        connect_async(self.url(TOKEN)).await.unwrap().0
    }
    async fn exited(&mut self) {
        timeout(Duration::from_secs(15), async {
            loop {
                if let Some(status) = self.child.try_wait().unwrap() {
                    assert!(status.success(), "{status}");
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("motor deve encerrar");
        let mut extra = String::new();
        assert_eq!(self.stdout.read_line(&mut extra).unwrap(), 0, "stdout contém apenas ready");
        let diagnostics = std::fs::read_to_string(&self.stderr).unwrap();
        assert!(!diagnostics.contains("audio_underrun"), "{diagnostics}");
    }
}
impl Drop for Motor {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

async fn next(socket: &mut Socket) -> Message {
    timeout(Duration::from_secs(30), socket.next()).await.expect("prazo de resposta").expect("conexão aberta").expect("mensagem válida")
}
async fn request(socket: &mut Socket, id: u64, cmd: &str, params: Value) -> Value {
    socket.send(Message::Text(json!({"id":id,"cmd":cmd,"params":params}).to_string().into())).await.unwrap();
    loop {
        if let Message::Text(text) = next(socket).await {
            let value: Value = serde_json::from_str(&text).unwrap();
            if value["id"] == id {
                return value;
            }
        }
    }
}
fn fixture() -> String {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/padrao.mp4").to_string_lossy().into_owned()
}

#[tokio::test]
async fn authentication_replacement_errors_and_shutdown() {
    let mut motor = Motor::start();
    assert!(connect_async(motor.url("errado")).await.is_err());
    assert!(connect_async(format!("ws://127.0.0.1:{}/", motor.port)).await.is_err());
    let mut first = motor.connect().await;
    let hello = request(&mut first, 1, "hello", json!({})).await;
    assert_eq!(hello["ok"], true);
    assert_eq!(hello["result"]["speech"], true);
    let encoders = hello["result"]["encoders"].as_array().unwrap();
    assert_eq!(encoders.first().unwrap(), "software");
    assert!(encoders.iter().all(|e| ["software", "nvenc_h264", "nvenc_hevc"].iter().any(|name| e == name)));
    // Um token errado não derruba a conexão autenticada.
    assert!(connect_async(motor.url("errado")).await.is_err());
    assert_eq!(request(&mut first, 2, "hello", json!({})).await["ok"], true);
    let mut second = motor.connect().await;
    timeout(Duration::from_secs(3), async {
        loop {
            match first.next().await {
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break,
                _ => {}
            }
        }
    })
    .await
    .expect("segunda conexão derruba a primeira");
    for (id, cmd, params, code) in [
        (3, "transport.play", json!({}), "no_session"),
        (4, "transport.seek", json!({"seconds":0}), "no_session"),
        (5, "media.probe", json!({"path":"arquivo-inexistente.mp4"}), "media_not_found"),
        (6, "preview.config", json!({"maxWidth":0,"maxHeight":1080,"quality":80}), "invalid_params"),
        (7, "preview.config", json!({"maxWidth":1920,"maxHeight":1080,"quality":101}), "invalid_params"),
        (8, "speech.unknown", json!({}), "unknown_command"),
        (9, "session.open", json!({"path":fixture(),"format":"errado"}), "invalid_params"),
        (10, "media.probe", json!({"path":"https://example.invalid/video.mp4"}), "invalid_params"),
    ] {
        let reply = request(&mut second, id, cmd, params).await;
        assert_eq!(reply["ok"], false, "{reply}");
        assert_eq!(reply["error"]["code"], code);
        assert!(reply["error"]["message"].as_str().unwrap().ends_with('.'));
    }
    second.send(Message::Text("{".into())).await.unwrap();
    assert!(matches!(next(&mut second).await, Message::Text(text) if text.contains("invalid_json")));
    assert_eq!(request(&mut second, 11, "shutdown", json!({})).await["ok"], true);
    motor.exited().await;
}

#[tokio::test]
async fn eof_stops_process_even_with_client_connected() {
    let mut motor = Motor::start();
    let _socket = motor.connect().await;
    drop(motor.child.stdin.take());
    motor.exited().await;
}

async fn frame(socket: &mut Socket, seconds: f64) -> (u32, u32) {
    loop {
        let Message::Binary(bytes) = next(socket).await else {
            continue;
        };
        assert_eq!(&bytes[..4], b"DVF1");
        assert_eq!(&bytes[12..16], &[0; 4]);
        let time = f64::from_le_bytes(bytes[16..24].try_into().unwrap());
        if (time - seconds).abs() > 0.001 {
            continue;
        }
        let width = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        let height = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
        let image = image::load_from_memory(&bytes[24..]).unwrap();
        assert_eq!((image.width(), image.height()), (width, height));
        return (width, height);
    }
}

#[tokio::test]
async fn probe_open_formats_config_and_paused_seek_send_real_jpeg() {
    let mut motor = Motor::start();
    let mut socket = motor.connect().await;
    let probe = request(&mut socket, 1, "media.probe", json!({"path":fixture()})).await;
    assert_eq!(probe["ok"], true, "{probe}");
    assert_eq!(probe["result"]["width"], 64);
    assert_eq!(probe["result"]["height"], 36);
    assert_eq!(probe["result"]["hasAudio"], true);
    assert!((probe["result"]["duration"].as_f64().unwrap() - 3.0).abs() < 0.1);
    for (index, format, width, height) in [(0, "source", 64, 36), (1, "vertical", 1080, 1920), (2, "horizontal", 1920, 1080)] {
        let open = request(&mut socket, 10 + index, "session.open", json!({"path":fixture(),"format":format})).await;
        assert_eq!(open["ok"], true, "{open}");
        assert_eq!(open["result"]["width"], width);
        assert_eq!(open["result"]["height"], height);
        assert_eq!(request(&mut socket, 20 + index, "preview.config", json!({"maxWidth":128,"maxHeight":128,"quality":75})).await["ok"], true);
        assert_eq!(request(&mut socket, 30 + index, "transport.seek", json!({"seconds":1.5})).await["ok"], true);
        let (w, h) = frame(&mut socket, 1.5).await;
        assert!(w <= 128 && h <= 128);
        assert_eq!(request(&mut socket, 40 + index, "transport.seek", json!({"seconds":-1.0})).await["error"]["code"], "invalid_params");
        assert_eq!(request(&mut socket, 50 + index, "transport.seek", json!({"seconds":4.0})).await["error"]["code"], "invalid_params");
    }
    assert_eq!(request(&mut socket, 99, "shutdown", json!({})).await["ok"], true);
    motor.exited().await;
}

#[tokio::test]
#[ignore = "Requer dispositivo de áudio padrão; reproduz tom sintético de baixo volume."]
async fn playback_audio_clock_pause_resume_seek_and_end() {
    let mut motor = Motor::start();
    let mut socket = motor.connect().await;
    assert_eq!(request(&mut socket, 1, "session.open", json!({"path":fixture(),"format":"source"})).await["ok"], true);
    let play = request(&mut socket, 2, "transport.play", json!({})).await;
    assert_eq!(play["ok"], true, "{play}");
    let start = Instant::now();
    let mut saw_stats = false;
    let mut last_seconds = 0.0;
    while start.elapsed() < Duration::from_secs(2) {
        if let Message::Text(text) = next(&mut socket).await {
            let event: Value = serde_json::from_str(&text).unwrap();
            if event["event"] == "stats" {
                saw_stats = true;
                assert!(event["data"]["fps"].as_f64().unwrap() > 0.0);
            }
            if event["event"] == "state" {
                let seconds = event["data"]["seconds"].as_f64().unwrap();
                assert!(seconds >= last_seconds);
                last_seconds = seconds;
            }
        }
    }
    assert!(saw_stats && last_seconds > 1.0);
    assert_eq!(request(&mut socket, 3, "transport.pause", json!({})).await["ok"], true);
    assert_eq!(request(&mut socket, 4, "transport.seek", json!({"seconds":0.5})).await["ok"], true);
    frame(&mut socket, 0.5).await;
    assert_eq!(request(&mut socket, 5, "transport.play", json!({})).await["ok"], true);
    assert_eq!(request(&mut socket, 6, "transport.seek", json!({"seconds":2.5})).await["ok"], true);
    loop {
        if let Message::Text(text) = next(&mut socket).await {
            let event: Value = serde_json::from_str(&text).unwrap();
            if event["event"] == "state" && event["data"]["playing"] == false {
                assert!((event["data"]["seconds"].as_f64().unwrap() - 3.0).abs() < 0.1);
                break;
            }
        }
    }
    assert_eq!(request(&mut socket, 99, "shutdown", json!({})).await["ok"], true);
    motor.exited().await;
}

#[tokio::test]
#[ignore = "Medição física 1080p; informe MOTOR_BENCH_MEDIA e opcionalmente MOTOR_BENCH_SECONDS."]
async fn benchmark_1080p_audio_and_seek() {
    let path = std::env::var("MOTOR_BENCH_MEDIA").expect("MOTOR_BENCH_MEDIA deve apontar para um MP4 sintético 1080p");
    let seconds: u64 = std::env::var("MOTOR_BENCH_SECONDS").ok().and_then(|s| s.parse().ok()).unwrap_or(10);
    let mut motor = Motor::start();
    let mut socket = motor.connect().await;
    let hello = request(&mut socket, 1, "hello", json!({})).await;
    eprintln!("GPU: {}", hello["result"]["gpu"]);
    let open = request(&mut socket, 2, "session.open", json!({"path":path,"format":"source"})).await;
    assert_eq!(open["ok"], true, "{open}");
    assert_eq!(open["result"]["width"], 1920);
    assert_eq!(open["result"]["height"], 1080);
    assert_eq!(request(&mut socket, 3, "preview.config", json!({"maxWidth":1920,"maxHeight":1080,"quality":80})).await["ok"], true);
    frame(&mut socket, 0.0).await;
    assert_eq!(request(&mut socket, 4, "transport.play", json!({})).await["ok"], true);
    let started = Instant::now();
    let mut received = 0u64;
    let mut first_frame = None;
    let mut last_frame = Instant::now();
    let mut last_seconds = 0.0;
    let mut last_stats = json!({});
    while started.elapsed() < Duration::from_secs(seconds) {
        match next(&mut socket).await {
            Message::Binary(bytes) => {
                assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 1920);
                assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), 1080);
                first_frame.get_or_insert_with(Instant::now);
                last_frame = Instant::now();
                received += 1;
            }
            Message::Text(text) => {
                let event: Value = serde_json::from_str(&text).unwrap();
                if event["event"] == "stats" {
                    last_stats = event["data"].clone();
                }
                if event["event"] == "state" {
                    last_seconds = event["data"]["seconds"].as_f64().unwrap();
                }
            }
            _ => {}
        }
    }
    let fps = (received.saturating_sub(1)) as f64 / last_frame.duration_since(first_frame.expect("quadros recebidos")).as_secs_f64();
    eprintln!("Prévia 1080p: {received} quadros; {fps:.2} qps; relógio: {last_seconds:.3}s; stats: {last_stats}");
    assert_eq!(request(&mut socket, 5, "transport.pause", json!({})).await["ok"], true);
    let mut seek_ms = Vec::new();
    let duration = open["result"]["duration"].as_f64().unwrap();
    for (id, time) in [(6, duration * 0.5), (7, duration * 0.9), (8, duration * 0.1)] {
        let start = Instant::now();
        assert_eq!(request(&mut socket, id, "transport.seek", json!({"seconds":time})).await["ok"], true);
        frame(&mut socket, time).await;
        seek_ms.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    eprintln!("Seek até JPEG (ms): {seek_ms:?}");
    assert_eq!(request(&mut socket, 99, "shutdown", json!({})).await["ok"], true);
    motor.exited().await;
    assert!(fps >= 29.5, "prévia abaixo de 30 qps: {fps}");
    assert!(last_seconds >= seconds as f64 - 0.5, "relógio de áudio atrasado");
    assert!(seek_ms.iter().all(|ms| *ms < 300.0), "seek excede 300 ms");
}

fn export_fixture() -> String {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/exportacao.mp4").to_string_lossy().into_owned()
}

async fn job_finished(socket: &mut Socket, job: &Value) -> Value {
    let mut fraction = 0.0;
    let mut progress = false;
    let deadline = Instant::now() + Duration::from_secs(600);
    loop {
        assert!(Instant::now() < deadline, "exportação excedeu prazo");
        let Message::Text(text) = next(socket).await else {
            continue;
        };
        let event: Value = serde_json::from_str(&text).unwrap();
        if event["data"]["jobId"] != *job {
            continue;
        }
        assert_eq!(event["data"]["kind"], "export");
        match event["event"].as_str() {
            Some("job.progress") => {
                let current = event["data"]["fraction"].as_f64().unwrap();
                assert!(current >= fraction && current <= 1.0);
                assert!(event["data"]["etaSeconds"].is_null() || event["data"]["etaSeconds"].as_f64().unwrap() >= 0.0);
                fraction = current;
                progress = true;
            }
            Some("job.done") => {
                assert!(progress);
                assert_eq!(fraction, 1.0);
                return event;
            }
            Some("job.error") => return event,
            _ => panic!("evento inesperado: {event}"),
        }
    }
}

#[tokio::test]
async fn export_all_presets_and_encoders_produce_media_with_audio() {
    let mut motor = Motor::start();
    let mut socket = motor.connect().await;
    assert_eq!(request(&mut socket, 1, "session.open", json!({"path":export_fixture(),"format":"source"})).await["ok"], true);
    for (preset, width, height) in [("shorts_1080x1920", 1080, 1920), ("youtube_1080p", 1920, 1080), ("youtube_2160p", 3840, 2160)] {
        for encoder in ["software", "nvenc_h264", "nvenc_hevc"] {
            let path = motor._dir.path().join(format!("{preset}-{encoder}.mp4"));
            let started = request(&mut socket, 2, "export.start", json!({"path":path,"preset":preset,"encoder":encoder})).await;
            assert_eq!(started["ok"], true, "{started}");
            let done = job_finished(&mut socket, &started["result"]["jobId"]).await;
            assert_eq!(done["event"], "job.done", "{done}");
            assert_eq!(done["data"]["result"]["path"], path.to_string_lossy().as_ref());
            assert_eq!(done["data"]["result"]["bytes"].as_u64().unwrap(), std::fs::metadata(&path).unwrap().len());
            let probe = request(&mut socket, 3, "media.probe", json!({"path":path})).await;
            assert_eq!(probe["ok"], true, "{probe}");
            assert_eq!(probe["result"]["width"], width);
            assert_eq!(probe["result"]["height"], height);
            assert_eq!(probe["result"]["hasAudio"], true);
            assert!((probe["result"]["duration"].as_f64().unwrap() - 0.4).abs() < 0.1);
        }
    }
    assert!(std::fs::read_dir(motor._dir.path().join("cache")).unwrap().next().is_none());
    request(&mut socket, 99, "shutdown", json!({})).await;
    motor.exited().await;
}

#[tokio::test]
async fn export_validation_cancellation_and_shutdown_preserve_files() {
    let mut motor = Motor::start();
    let mut socket = motor.connect().await;
    let path = motor._dir.path().join("cancelado.mp4");
    let params = json!({"path":path,"preset":"youtube_2160p","encoder":"software"});
    assert_eq!(request(&mut socket, 1, "export.start", params.clone()).await["error"]["code"], "no_session");
    assert_eq!(request(&mut socket, 2, "session.open", json!({"path":fixture(),"format":"source"})).await["ok"], true);
    for bad in [json!({"path":path,"preset":"outro","encoder":"software"}), json!({"path":path,"preset":"youtube_1080p","encoder":"outro"})] {
        assert_eq!(request(&mut socket, 3, "export.start", bad).await["error"]["code"], "invalid_params");
    }
    let existing = motor._dir.path().join("existente.mp4");
    std::fs::write(&existing, b"preservar").unwrap();
    assert_eq!(
        request(&mut socket, 4, "export.start", json!({"path":existing,"preset":"youtube_1080p","encoder":"software"})).await["error"]["code"],
        "output_exists"
    );
    let start = request(&mut socket, 5, "export.start", params.clone()).await;
    assert_eq!(start["ok"], true, "{start}");
    assert_eq!(request(&mut socket, 6, "export.start", params.clone()).await["error"]["code"], "export_busy");
    assert_eq!(request(&mut socket, 7, "job.cancel", json!({"jobId":0})).await["error"]["code"], "job_not_found");
    assert_eq!(request(&mut socket, 8, "job.cancel", json!({"jobId":start["result"]["jobId"]})).await["ok"], true);
    let cancelled = job_finished(&mut socket, &start["result"]["jobId"]).await;
    assert_eq!(cancelled["event"], "job.error", "{cancelled}");
    assert_eq!(cancelled["data"]["error"]["code"], "job_cancelled");
    assert!(!path.exists());
    assert!(std::fs::read_dir(motor._dir.path().join("cache")).unwrap().next().is_none());
    assert_eq!(std::fs::read(&existing).unwrap(), b"preservar");
    assert_eq!(request(&mut socket, 9, "export.start", params).await["ok"], true);
    request(&mut socket, 99, "shutdown", json!({})).await;
    motor.exited().await;
    assert!(!path.exists());
    assert!(std::fs::read_dir(motor._dir.path().join("cache")).unwrap().next().is_none());
}

#[tokio::test]
#[ignore = "Exporta um minuto por modelo no PC; requer MOTOR_EXPORT_MEDIA e MOTOR_EXPORT_DIR."]
async fn acceptance_export_one_minute_each_preset() {
    let input = std::env::var("MOTOR_EXPORT_MEDIA").expect("MOTOR_EXPORT_MEDIA");
    let output = std::path::PathBuf::from(std::env::var("MOTOR_EXPORT_DIR").expect("MOTOR_EXPORT_DIR"));
    std::fs::create_dir_all(&output).unwrap();
    let mut motor = Motor::start();
    let mut socket = motor.connect().await;
    let hello = request(&mut socket, 1, "hello", json!({})).await;
    eprintln!("Codificadores detectados: {}", hello["result"]["encoders"]);
    let open = request(&mut socket, 2, "session.open", json!({"path":input,"format":"source"})).await;
    assert_eq!(open["ok"], true, "{open}");
    assert!((open["result"]["duration"].as_f64().unwrap() - 60.0).abs() < 0.1);
    for (preset, width, height) in [("shorts_1080x1920", 1080, 1920), ("youtube_1080p", 1920, 1080), ("youtube_2160p", 3840, 2160)] {
        let path = output.join(format!("{preset}.mp4"));
        let now = Instant::now();
        let start = request(&mut socket, 3, "export.start", json!({"path":path,"preset":preset,"encoder":"nvenc_h264"})).await;
        assert_eq!(start["ok"], true, "{start}");
        let done = job_finished(&mut socket, &start["result"]["jobId"]).await;
        assert_eq!(done["event"], "job.done", "{done}");
        eprintln!("{preset}: {:.3} s; {} bytes", now.elapsed().as_secs_f64(), done["data"]["result"]["bytes"]);
        let probe = request(&mut socket, 4, "media.probe", json!({"path":path})).await;
        assert_eq!(probe["result"]["width"], width);
        assert_eq!(probe["result"]["height"], height);
        assert_eq!(probe["result"]["hasAudio"], true);
        assert!((probe["result"]["duration"].as_f64().unwrap() - 60.0).abs() < 0.1);
    }
    request(&mut socket, 99, "shutdown", json!({})).await;
    motor.exited().await;
    eprintln!("{}", std::fs::read_to_string(&motor.stderr).unwrap());
}

#[tokio::test]
async fn speech_catalogue_validation_never_downloads_implicitly() {
    let mut motor = Motor::start();
    let mut socket = motor.connect().await;
    let listed = request(&mut socket, 1, "speech.models", json!({})).await;
    let models = listed["result"]["models"].as_array().unwrap();
    assert_eq!(models.len(), 3);
    for (model, id) in models.iter().zip(["whisper-tiny", "whisper-base", "whisper-small"]) {
        assert_eq!(model["id"], id);
        assert_eq!(model["installed"], false);
        assert!(model["sizeBytes"].as_u64().unwrap() > 100_000_000);
        assert!(model["license"].as_str().unwrap().contains("MIT"));
    }
    assert!(!motor._dir.path().join("modelos").exists());
    assert_eq!(request(&mut socket, 2, "speech.download", json!({"model":"../escape"})).await["error"]["code"], "unknown_model");
    assert_eq!(request(&mut socket, 3, "speech.transcribe", json!({"model":"whisper-small","language":"pt"})).await["error"]["code"], "no_session");
    request(&mut socket, 4, "session.open", json!({"path":fixture(),"format":"source"})).await;
    assert_eq!(request(&mut socket, 5, "speech.transcribe", json!({"model":"whisper-small","language":"pt"})).await["error"]["code"], "model_not_installed");
    assert_eq!(request(&mut socket, 6, "speech.transcribe", json!({"model":"whisper-small","language":"fr"})).await["error"]["code"], "invalid_params");
    assert!(!motor._dir.path().join("modelos").exists());
    // Presença dos nomes dos arquivos não basta: o worker deve verificar SHA-256 antes de carregar.
    let corrupt = motor._dir.path().join("modelos/whisper-small");
    std::fs::create_dir_all(&corrupt).unwrap();
    for name in ["config.json", "generation_config.json", "tokenizer.json", "model.safetensors"] {
        std::fs::write(corrupt.join(name), b"invalido").unwrap();
    }
    let started = request(&mut socket, 7, "speech.transcribe", json!({"model":"whisper-small","language":"pt"})).await;
    assert_eq!(started["ok"], true, "{started}");
    let done = speech_job_finished(&mut socket, &started["result"]["jobId"], "transcribe", Duration::from_secs(10)).await;
    assert_eq!(done["event"], "job.error");
    assert_eq!(done["data"]["error"]["code"], "model_not_installed");
    request(&mut socket, 99, "shutdown", json!({})).await;
    motor.exited().await;
}

async fn speech_job_finished(socket: &mut Socket, job: &Value, kind: &str, deadline: Duration) -> Value {
    let until = Instant::now() + deadline;
    let mut last = 0.0;
    loop {
        assert!(Instant::now() < until, "job de fala excedeu o prazo");
        let Message::Text(text) = next(socket).await else {
            continue;
        };
        let event: Value = serde_json::from_str(&text).unwrap();
        if event["data"]["jobId"] != *job {
            continue;
        }
        assert_eq!(event["data"]["kind"], kind);
        if event["event"] == "job.progress" {
            let fraction = event["data"]["fraction"].as_f64().unwrap();
            assert!(fraction >= last && fraction <= 1.0);
            last = fraction;
        } else if event["event"] == "job.done" || event["event"] == "job.error" {
            return event;
        }
    }
}

#[tokio::test]
#[ignore = "Baixa ~969,5 MB após autorização explícita e mede Whisper small com fala PT-BR local de 60 s."]
async fn acceptance_whisper_small_portuguese_one_minute() {
    assert_eq!(
        std::env::var("MOTOR_SPEECH_DOWNLOAD_CONFIRMED").as_deref(),
        Ok("1"),
        "confirme download, tamanho, origem Hugging Face/OpenAI e licenças MIT/Apache-2.0"
    );
    let root = std::path::PathBuf::from(std::env::var("MOTOR_SPEECH_DATA_DIR").expect("MOTOR_SPEECH_DATA_DIR"));
    let media = std::env::var("MOTOR_SPEECH_MEDIA").expect("MOTOR_SPEECH_MEDIA");
    let mut motor = Motor::start_with_data_dir(Some(&root));
    let mut socket = motor.connect().await;
    let download_time = Instant::now();
    let started = request(&mut socket, 1, "speech.download", json!({"model":"whisper-small"})).await;
    assert_eq!(started["ok"], true, "{started}");
    let done = speech_job_finished(&mut socket, &started["result"]["jobId"], "download", Duration::from_secs(3600)).await;
    assert_eq!(done["event"], "job.done", "{done}");
    eprintln!("Download/verificação small: {:.3} s", download_time.elapsed().as_secs_f64());
    let listed = request(&mut socket, 2, "speech.models", json!({})).await;
    assert_eq!(listed["result"]["models"][2]["installed"], true);
    let open = request(&mut socket, 3, "session.open", json!({"path":media,"format":"source"})).await;
    assert_eq!(open["ok"], true, "{open}");
    assert!((open["result"]["duration"].as_f64().unwrap() - 60.0).abs() < 0.1);
    let now = Instant::now();
    let started = request(&mut socket, 4, "speech.transcribe", json!({"model":"whisper-small","language":"pt"})).await;
    assert_eq!(started["ok"], true, "{started}");
    let done = speech_job_finished(&mut socket, &started["result"]["jobId"], "transcribe", Duration::from_secs(1800)).await;
    let elapsed = now.elapsed().as_secs_f64();
    assert_eq!(done["event"], "job.done", "{done}");
    let result = &done["data"]["result"];
    assert_eq!(result["language"], "pt");
    let words = result["words"].as_array().unwrap();
    assert!(words.len() > 30, "poucas palavras: {result}");
    let mut end = 0.0;
    for word in words {
        let start = word["start"].as_f64().unwrap();
        let finish = word["end"].as_f64().unwrap();
        assert!(start >= end && finish > start && finish <= 60.1);
        assert!((0.0..=1.0).contains(&word["confidence"].as_f64().unwrap()));
        end = finish;
    }
    std::fs::write(root.join("resultado-pt-br.json"), serde_json::to_vec_pretty(result).unwrap()).unwrap();
    eprintln!("Whisper small PT-BR 60 s: {elapsed:.3} s; {} palavras", words.len());
    request(&mut socket, 99, "shutdown", json!({})).await;
    motor.exited().await;
    assert!(elapsed < 60.0, "transcrição excedeu a meta de 60 s: {elapsed:.3} s");
}

#[tokio::test]
#[ignore = "Acessa Hugging Face após confirmação para verificar cancelamento durante transferência real."]
async fn acceptance_cancel_small_download() {
    assert_eq!(std::env::var("MOTOR_SPEECH_DOWNLOAD_CONFIRMED").as_deref(), Ok("1"));
    let base = std::path::PathBuf::from(std::env::var("MOTOR_SPEECH_DATA_DIR").expect("MOTOR_SPEECH_DATA_DIR"));
    let root = base.join("teste-cancelamento");
    let mut motor = Motor::start_with_data_dir(Some(&root));
    let mut socket = motor.connect().await;
    let started = request(&mut socket, 1, "speech.download", json!({"model":"whisper-small"})).await;
    assert_eq!(started["ok"], true, "{started}");
    let job = &started["result"]["jobId"];
    loop {
        let Message::Text(text) = next(&mut socket).await else {
            continue;
        };
        let event: Value = serde_json::from_str(&text).unwrap();
        if event["data"]["jobId"] != *job {
            continue;
        }
        assert_ne!(event["event"], "job.error", "{event}");
        assert_ne!(event["event"], "job.done", "o modelo já estava instalado; use outra pasta de teste");
        if event["event"] == "job.progress" && event["data"]["fraction"].as_f64().unwrap() > 0.01 {
            break;
        }
    }
    let now = Instant::now();
    assert_eq!(request(&mut socket, 2, "job.cancel", json!({"jobId":job})).await["ok"], true);
    let done = speech_job_finished(&mut socket, job, "download", Duration::from_secs(60)).await;
    assert_eq!(done["event"], "job.error", "{done}");
    assert_eq!(done["data"]["error"]["code"], "job_cancelled");
    eprintln!("Cancelamento de download real: {:.3} s", now.elapsed().as_secs_f64());
    let model_dir = root.join("modelos/whisper-small");
    assert!(std::fs::read_dir(&model_dir).unwrap().all(|entry| !entry.unwrap().file_name().to_string_lossy().ends_with(".part")));
    let listed = request(&mut socket, 3, "speech.models", json!({})).await;
    assert_eq!(listed["result"]["models"][2]["installed"], false);
    request(&mut socket, 99, "shutdown", json!({})).await;
    motor.exited().await;
}
