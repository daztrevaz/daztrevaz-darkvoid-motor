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
mod audio_cases;
mod caption_cases;
mod loudness_cases;
mod media_cases;
mod timeline_cases;
const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

#[tokio::test]
#[ignore = "Aceite M3.2: exporta 1 h completa em 1080p pelo processo real; MOTOR_M32_OUTPUT recebe os artefatos sintéticos."]
async fn color_one_hour_1080p_export_via_real_process() {
    let output = std::path::PathBuf::from(std::env::var("MOTOR_M32_OUTPUT").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    let dir = tempfile::Builder::new().prefix("m32-").tempdir_in(output).unwrap();
    let image = dir.path().join("fonte.png");
    image::RgbImage::from_fn(96, 54, |x, y| image::Rgb([(20 + x * 2) as u8, (20 + y * 4) as u8, if (x / 8 + y / 8) % 2 == 0 { 40 } else { 210 }]))
        .save(&image)
        .unwrap();
    let lut = dir.path().join("cor.cube");
    let mut text = String::from("LUT_3D_SIZE 2\n");
    for b in 0..2 {
        for g in 0..2 {
            for r in 0..2 {
                text.push_str(&format!("{} {g} {b}\n", 1 - r));
            }
        }
    }
    std::fs::write(&lut, text).unwrap();
    let mut motor = Motor::start_with_data_dir(Some(&dir.path().join("dados")));
    let mut socket = motor.connect().await;
    let hello = request(&mut socket, 1, "hello", json!({})).await;
    let catalog = request(&mut socket, 2, "catalog.get", json!({})).await;
    let looks: Vec<_> = catalog["result"]["items"].as_array().unwrap().iter().filter(|i| i["kind"] == "look").map(|i| i["id"].clone()).collect();
    let groups = [
        json!({"exposure":0.7}),
        json!({"faded_film":35,"vibrance":40}),
        json!({"curve_luma":[[0,0],[0.5,0.7],[1,1]]}),
        json!({"wheel_midtones":[0.25,-0.1]}),
        json!({"hsl_on":true,"hsl_hue":30,"hsl_hue_range":100,"hsl_sat":20}),
        json!({"vignette_amount":-3}),
    ];
    let clips:Vec<_>=(0..120).map(|i|{
        let mut color=groups[i%6].clone();if i%12==0 {color["lut"]=json!(lut);}
        let mut c=json!({"id":format!("c{i}"),"media":image,"track":"V1","start":i*30,"in":0,"out":30,"volume":1,"fadeIn":0,"fadeOut":0,
            "color":{"enabled":true,"params":color},"look":{"id":looks[i%looks.len()],"intensity":50}});
        if (1..=5).contains(&i) {c["transitionIn"]=json!({"id":(["transition.cross_dissolve","transition.push","transition.flash","transition.linear_wipe","transition.glitch"][i-1]),"duration":1});}c
    }).collect();
    let timeline = request(&mut socket, 3, "timeline.set", json!({"format":"horizontal","fps":1,"clips":clips})).await;
    assert_eq!(timeline["ok"], true, "{timeline}");
    assert_eq!(timeline["result"]["duration"], 3600.0);
    let encoder = if hello["result"]["encoders"].as_array().unwrap().iter().any(|e| e == "nvenc_h264") { "nvenc_h264" } else { "software" };
    let target = dir.path().join("uma-hora.mp4");
    let start = Instant::now();
    let job = request(&mut socket, 4, "export.start", json!({"path":target,"preset":"youtube_1080p","encoder":encoder})).await;
    assert_eq!(job["ok"], true, "{job}");
    let done = job_finished(&mut socket, &job["result"]["jobId"]).await;
    assert_eq!(done["event"], "job.done", "{done}");
    let elapsed = start.elapsed().as_secs_f64();
    let probe = request(&mut socket, 5, "media.probe", json!({"path":target})).await;
    assert_eq!(probe["ok"], true, "{probe}");
    assert_eq!(probe["result"]["duration"], 3600.0);
    assert_eq!(probe["result"]["width"], 1920);
    assert_eq!(probe["result"]["height"], 1080);
    assert_eq!(probe["result"]["fps"], 1.0);
    let report = json!({"hello":hello["result"],"preset":"youtube_1080p","encoderRequested":encoder,"clips":120,"colorGroups":6,"looks":26,"lookIntensity":50,
        "transitions":5,"exportSeconds":elapsed,"done":done,"probe":probe["result"]});
    std::fs::write(dir.path().join("relatorio.json"), serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    request(&mut socket, 99, "shutdown", json!({})).await;
    motor.exited().await;
    let path = dir.keep();
    eprintln!("M3.2 processo real: {elapsed:.3}s; artefatos {}", path.display());
}

#[tokio::test]
async fn color_looks_lut_errors_and_scopes_via_websocket() {
    use base64::Engine;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cor.png");
    image::RgbImage::from_fn(96, 54, |x, y| image::Rgb([(x * 2 + 20) as u8, (y * 4 + 20) as u8, 90])).save(&path).unwrap();
    let mut motor = Motor::start();
    let mut socket = motor.connect().await;
    assert_eq!(request(&mut socket, 1, "scopes.get", json!({"kinds":["waveform"]})).await["error"]["code"], "no_session");
    let catalog = request(&mut socket, 2, "catalog.get", json!({})).await;
    let items = catalog["result"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 160);
    assert_eq!(items.iter().filter(|i| i["kind"] == "look").count(), 26);
    assert!(items.iter().any(|i| i["id"] == "color.grade"));
    assert_eq!(request(&mut socket, 3, "preview.config", json!({"maxWidth":96,"maxHeight":54,"quality":95})).await["ok"], true);
    let mut params = json!({"format":"horizontal","clips":[{"id":"cor","media":path,"track":"V1","start":0,"in":0,"out":1,"volume":1,"fadeIn":0,"fadeOut":0}]});
    let before = command_frame(&mut socket, 4, "timeline.set", params.clone()).await;
    params["clips"][0]["color"] = json!({"enabled":true,"params":{"exposure":0.8,"curve_luma":[[0,0],[0.5,0.6],[1,1]]}});
    params["clips"][0]["look"] = json!({"id":"look.preset.sepia","intensity":50});
    let after = command_frame(&mut socket, 5, "timeline.set", params.clone()).await;
    assert_ne!(before, after);
    let scopes = request(&mut socket, 6, "scopes.get", json!({"kinds":["waveform","vectorscope"]})).await;
    assert_eq!(scopes["ok"], true, "{scopes}");
    for kind in ["waveform", "vectorscope"] {
        let png = base64::engine::general_purpose::STANDARD.decode(scopes["result"][kind].as_str().unwrap()).unwrap();
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
        image::load_from_memory(&png).unwrap();
    }
    let lut = dir.path().join("ruim.cube");
    std::fs::write(&lut, "LUT_3D_SIZE 66\n").unwrap();
    params["clips"][0]["color"]["params"]["lut"] = json!(lut);
    assert_eq!(request(&mut socket, 7, "timeline.set", params.clone()).await["error"]["code"], "lut_invalid");
    std::fs::remove_file(&lut).unwrap();
    assert_eq!(request(&mut socket, 8, "timeline.set", params).await["error"]["code"], "media_not_found");
    assert_eq!(command_frame(&mut socket, 9, "transport.seek", json!({"seconds":0})).await, after, "validação preserva sessão");
    request(&mut socket, 99, "shutdown", json!({})).await;
    motor.exited().await;
}

#[tokio::test]
async fn catalog_transitions_render_via_timeline_set() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.png");
    let b = dir.path().join("b.png");
    for (path, reverse) in [(&a, false), (&b, true)] {
        image::RgbImage::from_fn(96, 54, |x, y| {
            let x = if reverse { 95 - x } else { x };
            image::Rgb([(20 + x * 2) as u8, (20 + y * 4) as u8, if (x / 8 + y / 8) % 2 == 0 { 40 } else { 210 }])
        })
        .save(path)
        .unwrap();
    }
    let mut motor = Motor::start();
    let mut socket = motor.connect().await;
    assert_eq!(request(&mut socket, 1, "hello", json!({})).await["result"]["protocol"], 5);
    let catalog = request(&mut socket, 2, "catalog.get", json!({})).await;
    let items: Vec<_> = catalog["result"]["items"].as_array().unwrap().iter().filter(|item| item["kind"] == "transition").collect();
    assert_eq!(items.len(), 50);
    assert_eq!(request(&mut socket, 3, "preview.config", json!({"maxWidth":96,"maxHeight":54,"quality":95})).await["ok"], true);
    let base = json!({"format":"horizontal","clips":[
        {"id":"a","media":a,"track":"V1","start":0,"in":0,"out":2,"volume":1,"fadeIn":0,"fadeOut":0},
        {"id":"b","media":b,"track":"V1","start":2,"in":0,"out":2,"volume":1,"fadeIn":0,"fadeOut":0}
    ]});
    command_frame(&mut socket, 4, "timeline.set", base.clone()).await;
    let baseline = command_frame(&mut socket, 5, "transport.seek", json!({"seconds":1.8})).await;
    for item in items {
        let mut params = base.clone();
        params["clips"][1]["transitionIn"] = json!({"id":item["id"],"duration":1});
        let rendered = command_frame(&mut socket, 6, "timeline.set", params).await;
        assert_ne!(rendered, baseline, "{}", item["id"]);
    }
    request(&mut socket, 99, "shutdown", json!({})).await;
    motor.exited().await;
}

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
async fn audio_peaks_validation_silence_cache_and_file_change() {
    let mut motor = Motor::start();
    let mut socket = motor.connect().await;
    let absent = request(&mut socket, 1, "audio.peaks", json!({"buckets":20})).await;
    assert_eq!(absent["error"]["code"], "no_session");
    assert_eq!(absent["error"]["message"], "Abra um vídeo antes de calcular a onda de áudio.");
    let source = motor._dir.path().join("source.mp4");
    std::fs::copy(fixture(), &source).unwrap();
    assert_eq!(request(&mut socket, 2, "session.open", json!({"path":source,"format":"source"})).await["ok"], true);
    for buckets in [json!(0), json!(20001), json!(-1), json!(1.5), json!("20"), Value::Null] {
        assert_eq!(request(&mut socket, 3, "audio.peaks", json!({"buckets":buckets})).await["error"]["code"], "invalid_params");
    }
    let mut first = None;
    for buckets in [1, 20, 20000, 20] {
        let result = request(&mut socket, 4, "audio.peaks", json!({"buckets":buckets})).await;
        assert_eq!(result["ok"], true, "{result}");
        let peaks = result["result"]["peaks"].as_array().unwrap();
        assert_eq!(peaks.len(), buckets);
        assert!(peaks.iter().all(|v| (0.0..=1.0).contains(&v.as_f64().unwrap())));
        assert!(peaks.iter().any(|v| v.as_f64().unwrap() > 0.01));
        if buckets == 20 {
            if let Some(old) = &first {
                assert_eq!(&result, old);
            } else {
                first = Some(result);
            }
        }
    }
    let caches =
        || std::fs::read_dir(motor._dir.path().join("cache")).unwrap().flatten().filter(|f| f.file_name().to_string_lossy().starts_with("peaks-v1-")).count();
    assert_eq!(caches(), 1);
    let silent = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sem-audio.mp4");
    std::fs::copy(silent, &source).unwrap();
    let result = request(&mut socket, 5, "audio.peaks", json!({"buckets":20000})).await;
    assert_eq!(result["ok"], true, "{result}");
    assert!(result["result"]["peaks"].as_array().unwrap().iter().all(|v| v == 0.0));
    assert_eq!(caches(), 2);
    let multiple = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/dois-audios.mp4");
    assert_eq!(request(&mut socket, 6, "session.open", json!({"path":multiple,"format":"source"})).await["ok"], true);
    let result = request(&mut socket, 7, "audio.peaks", json!({"buckets":1})).await;
    assert_eq!(result["ok"], true, "{result}");
    assert!(result["result"]["peaks"][0].as_f64().unwrap() > 0.6, "deve incluir a segunda faixa de áudio");
    request(&mut socket, 99, "shutdown", json!({})).await;
    motor.exited().await;
}

#[tokio::test]
#[ignore = "M1.2: requer MOTOR_PEAKS_MEDIA sintético de 10 min, MOTOR_PEAKS_OUTPUT e áudio padrão."]
async fn audio_peaks_benchmark_ten_minutes() {
    let media = std::env::var("MOTOR_PEAKS_MEDIA").unwrap();
    let output = std::path::PathBuf::from(std::env::var("MOTOR_PEAKS_OUTPUT").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    let cache = output.join("single");
    std::fs::create_dir_all(&cache).unwrap();
    let mut motor = Motor::start_with_data_dir(Some(&cache));
    let mut socket = motor.connect().await;
    let open = request(&mut socket, 1, "session.open", json!({"path":media,"format":"source"})).await;
    assert_eq!(open["ok"], true, "{open}");
    assert!((open["result"]["duration"].as_f64().unwrap() - 600.0).abs() < 0.1);
    assert_eq!(open["result"]["width"], 1920);
    let mut timings = Vec::new();
    let mut reference = Value::Null;
    for (name, buckets) in [("cold", 20000), ("warm", 20000), ("newBuckets", 1973), ("warmAgain", 1973)] {
        let start = Instant::now();
        let response = request(&mut socket, 2, "audio.peaks", json!({"buckets":buckets})).await;
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["result"]["peaks"].as_array().unwrap().len(), buckets);
        assert!(response["result"]["peaks"].as_array().unwrap().iter().any(|p| p.as_f64().unwrap() > 0.25));
        if name == "cold" {
            reference = response["result"].clone();
        }
        timings.push(json!({"kind":name,"ms":ms,"buckets":buckets}));
        eprintln!("M1.2 {name}: {ms:.3} ms");
    }
    request(&mut socket, 99, "shutdown", json!({})).await;
    motor.exited().await;
    let files: Vec<_> = std::fs::read_dir(cache.join("cache")).unwrap().flatten().collect();
    assert_eq!(files.len(), 1);
    let cache_bytes = files[0].metadata().unwrap().len();
    let written = files[0].metadata().unwrap().modified().unwrap();
    let mut motor = Motor::start_with_data_dir(Some(&cache));
    let mut socket = motor.connect().await;
    request(&mut socket, 1, "session.open", json!({"path":media,"format":"source"})).await;
    let start = Instant::now();
    let response = request(&mut socket, 2, "audio.peaks", json!({"buckets":20000})).await;
    let disk_ms = start.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(response["result"], reference, "{response}");
    assert_eq!(files[0].metadata().unwrap().modified().unwrap(), written);
    eprintln!("M1.2 restart/disk: {disk_ms:.3} ms; cache: {cache_bytes} bytes");
    request(&mut socket, 99, "shutdown", json!({})).await;
    motor.exited().await;

    let concurrent = output.join("concurrent");
    std::fs::create_dir_all(&concurrent).unwrap();
    let mut motor = Motor::start_with_data_dir(Some(&concurrent));
    let mut socket = motor.connect().await;
    request(&mut socket, 1, "session.open", json!({"path":media,"format":"source"})).await;
    frame(&mut socket, 0.0).await;
    assert_eq!(request(&mut socket, 2, "transport.play", json!({})).await["ok"], true);
    let job = request(&mut socket, 3, "export.start", json!({"path":output.join("concurrent.mp4"),"preset":"youtube_1080p","encoder":"nvenc_h264"})).await;
    assert_eq!(job["ok"], true, "{job}");
    let start = Instant::now();
    for (id, cmd, params) in [(4, "audio.peaks", json!({"buckets":20000})), (5, "audio.peaks", json!({"buckets":1973})), (6, "hello", json!({}))] {
        socket.send(Message::Text(json!({"id":id,"cmd":cmd,"params":params}).to_string().into())).await.unwrap();
    }
    let mut done = 0;
    let mut frames = 0;
    let mut last = Instant::now();
    let mut gap = 0.0f64;
    let mut hello_ms = None;
    let mut peak_ms = Vec::new();
    let mut progress = 0;
    let mut audio_seconds = 0.0f64;
    while done < 2 || start.elapsed() < Duration::from_secs(3) || hello_ms.is_none() {
        match next(&mut socket).await {
            Message::Binary(_) => {
                frames += 1;
                let now = Instant::now();
                gap = gap.max(now.duration_since(last).as_secs_f64() * 1000.0);
                last = now;
            }
            Message::Text(text) => {
                let value: Value = serde_json::from_str(&text).unwrap();
                if value["id"] == 4 || value["id"] == 5 {
                    assert_eq!(value["ok"], true, "{value}");
                    done += 1;
                    peak_ms.push(start.elapsed().as_secs_f64() * 1000.0);
                }
                if value["id"] == 6 {
                    assert_eq!(value["ok"], true);
                    hello_ms = Some(start.elapsed().as_secs_f64() * 1000.0);
                }
                if value["event"] == "job.progress" {
                    progress += 1;
                }
                if value["event"] == "state" {
                    audio_seconds = audio_seconds.max(value["data"]["seconds"].as_f64().unwrap());
                }
            }
            _ => {}
        }
        assert!(start.elapsed() < Duration::from_secs(30));
    }
    request(&mut socket, 7, "transport.pause", json!({})).await;
    assert_eq!(request(&mut socket, 8, "job.cancel", json!({"jobId":job["result"]["jobId"]})).await["ok"], true);
    assert_eq!(job_finished(&mut socket, &job["result"]["jobId"]).await["event"], "job.error");
    request(&mut socket, 99, "shutdown", json!({})).await;
    motor.exited().await;
    assert_eq!(std::fs::read_dir(concurrent.join("cache")).unwrap().count(), 1, "cálculo compartilhado deve criar somente um cache");
    eprintln!(
        "M1.2 concurrent: peaks_ms={peak_ms:?}; hello_ms={hello_ms:?}; frames={frames}; max_gap_ms={gap:.3}; progress={progress}; audio_s={audio_seconds:.3}"
    );
    std::fs::write(
        output.join("measurements.json"),
        serde_json::to_vec_pretty(&json!({"timings":timings,"diskMs":disk_ms,"cacheBytes":cache_bytes,
        "concurrent":{"peaksMs":peak_ms,"helloMs":hello_ms,"frames":frames,"maxGapMs":gap,"exportProgressEvents":progress,"audioSeconds":audio_seconds}}))
        .unwrap(),
    )
    .unwrap();
    assert!(timings[0]["ms"].as_f64().unwrap() < 3000.0, "primeiro cálculo excede 3 s");
    assert!(timings[1]["ms"].as_f64().unwrap() < 50.0 && timings[2]["ms"].as_f64().unwrap() < 50.0, "cache não foi instantâneo");
    assert!(frames > 20 && gap < 300.0 && audio_seconds > 2.5 && progress > 0 && hello_ms.unwrap() < 100.0, "cálculo bloqueou prévia, comandos ou exportação");
}

async fn caption_frame(socket: &mut Socket, id: u64, params: Value) -> Vec<u8> {
    command_frame(socket, id, "captions.set", params).await
}
async fn command_frame(socket: &mut Socket, id: u64, cmd: &str, params: Value) -> Vec<u8> {
    socket.send(Message::Text(json!({"id":id,"cmd":cmd,"params":params}).to_string().into())).await.unwrap();
    let mut result = None;
    let mut ack = false;
    while !ack || result.is_none() {
        match next(socket).await {
            Message::Binary(bytes) => {
                result = Some(bytes[24..].to_vec());
            }
            Message::Text(text) => {
                let v: Value = serde_json::from_str(&text).unwrap();
                if v["id"] == id {
                    assert_eq!(v["ok"], true, "{v}");
                    ack = true;
                }
            }
            _ => {}
        }
    }
    result.unwrap()
}

#[tokio::test]
async fn captions_replace_clear_validation_and_srt() {
    let mut motor = Motor::start();
    let mut socket = motor.connect().await;
    assert_eq!(request(&mut socket, 1, "captions.set", caption_cases::captions("phrase", 3)).await["error"]["code"], "no_session");
    assert_eq!(request(&mut socket, 2, "captions.clear", json!({})).await["error"]["code"], "no_session");
    request(&mut socket, 3, "session.open", json!({"path":fixture(),"format":"horizontal"})).await;
    frame(&mut socket, 0.0).await;
    let mut images = Vec::new();
    request(&mut socket, 10, "transport.seek", json!({"seconds":0.08})).await;
    frame(&mut socket, 0.08).await;
    for mode in ["phrase", "word", "karaoke", "pop"] {
        let jpeg = caption_frame(&mut socket, 4, caption_cases::captions(mode, 3)).await;
        images.push(jpeg);
    }
    assert!(images.windows(2).all(|v| v[0] != v[1]));
    for bad in [
        json!({}),
        {
            let mut p = caption_cases::captions("phrase", 3);
            p["style"]["color"] = json!("#zzzzzz");
            p
        },
        {
            let mut p = caption_cases::captions("phrase", 3);
            p["cues"][0]["words"][1]["start"] = json!(0.1);
            p
        },
    ] {
        assert_eq!(request(&mut socket, 5, "captions.set", bad).await["error"]["code"], "captions_invalid");
    }
    let path = motor._dir.path().join("legendas.srt");
    assert_eq!(request(&mut socket, 6, "captions.exportSrt", json!({"path":path})).await["ok"], true);
    let srt = std::fs::read_to_string(&path).unwrap();
    assert!(srt.contains("00:00:00,000 --> 00:00:03,000"));
    assert!(srt.contains("Olá, São Paulo!"));
    assert_eq!(request(&mut socket, 7, "captions.exportSrt", json!({"path":path})).await["ok"], false);
    assert_eq!(request(&mut socket, 8, "captions.clear", json!({})).await["ok"], true);
    frame(&mut socket, 0.08).await;
    let empty = motor._dir.path().join("vazio.srt");
    assert_eq!(request(&mut socket, 9, "captions.exportSrt", json!({"path":empty})).await["ok"], true);
    assert_eq!(std::fs::read(empty).unwrap().len(), 0);
    request(&mut socket, 99, "shutdown", json!({})).await;
    motor.exited().await;
}

#[tokio::test]
#[ignore = "Medição M1.1: requer MOTOR_BENCH_MEDIA sintético de 60s, FFmpeg e áudio padrão."]
async fn captions_benchmark_one_minute() {
    let media = std::env::var("MOTOR_BENCH_MEDIA").unwrap();
    let output = std::path::PathBuf::from(std::env::var("MOTOR_CAPTION_OUTPUT").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    let mut motor = Motor::start();
    let mut socket = motor.connect().await;
    let hello = request(&mut socket, 1, "hello", json!({})).await;
    eprintln!("M1.1 GPU: {}", hello["result"]["gpu"]);
    let encoder = if hello["result"]["encoders"].as_array().unwrap().iter().any(|e| e == "nvenc_h264") { "nvenc_h264" } else { "software" };
    let open = request(&mut socket, 2, "session.open", json!({"path":media,"format":"source"})).await;
    assert_eq!(open["ok"], true, "{open}");
    assert_eq!(open["result"]["width"], 1920);
    frame(&mut socket, 0.0).await;
    let mut measurements = Vec::new();
    for mode in ["phrase", "word", "karaoke", "pop"] {
        request(&mut socket, 3, "transport.seek", json!({"seconds":0.0})).await;
        frame(&mut socket, 0.0).await;
        let mut redraws = Vec::new();
        for size in [54, 56, 58, 54, 55] {
            let mut p = caption_cases::captions(mode, 60);
            p["style"]["size"] = json!(size);
            let start = Instant::now();
            let jpeg = caption_frame(&mut socket, 4, p).await;
            redraws.push(start.elapsed().as_secs_f64() * 1000.0);
            std::fs::write(output.join(format!("{mode}-preview.jpg")), jpeg).unwrap();
        }
        let play = request(&mut socket, 5, "transport.play", json!({})).await;
        assert_eq!(play["ok"], true, "{play}");
        let start = Instant::now();
        let mut frames = Vec::new();
        let mut stats = Vec::new();
        while start.elapsed() < Duration::from_secs(5) {
            match next(&mut socket).await {
                Message::Binary(_) => frames.push(Instant::now()),
                Message::Text(text) => {
                    let v: Value = serde_json::from_str(&text).unwrap();
                    if v["event"] == "stats" {
                        stats.push(v["data"].clone());
                    }
                }
                _ => {}
            }
        }
        request(&mut socket, 6, "transport.pause", json!({})).await;
        let fps = (frames.len() - 1) as f64 / frames.last().unwrap().duration_since(frames[0]).as_secs_f64();
        let path = output.join(format!("{mode}.mp4"));
        let start = Instant::now();
        let job = request(&mut socket, 7, "export.start", json!({"path":path,"preset":"youtube_1080p","encoder":encoder,"burnCaptions":true})).await;
        assert_eq!(job["ok"], true, "{job}");
        let done = job_finished(&mut socket, &job["result"]["jobId"]).await;
        assert_eq!(done["event"], "job.done", "{done}");
        let export_seconds = start.elapsed().as_secs_f64();
        let sample = output.join(format!("{mode}-export.rgb"));
        let decoded = Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-i"])
            .arg(&path)
            .args(["-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgb24", "-y"])
            .arg(&sample)
            .status()
            .unwrap();
        assert!(decoded.success());
        let exported = std::fs::read(sample).unwrap();
        let preview = image::open(output.join(format!("{mode}-preview.jpg"))).unwrap().into_rgb8();
        assert_eq!(preview.as_raw().len(), exported.len());
        // Compare the caption region; independent H.264/JPEG compression permits small colour differences.
        let mut sum = 0u64;
        let mut count = 0u64;
        for y in 780..1040 {
            for x in 480..1440 {
                for c in 0..3 {
                    let i = (y * 1920 + x) * 3 + c;
                    sum += u64::from(preview.as_raw()[i].abs_diff(exported[i]));
                    count += 1;
                }
            }
        }
        let mut mae = sum as f64 / count as f64;
        let mut comparisons = vec![json!({"seconds":0.0,"mae":mae})];
        for number in [2u32, 5, 27, 32, 62, 90, 1799] {
            let seconds = f64::from(number) / 30.0;
            let jpeg = command_frame(&mut socket, 70, "transport.seek", json!({"seconds":seconds})).await;
            std::fs::write(output.join(format!("{mode}-{number}-preview.jpg")), &jpeg).unwrap();
            let preview = image::load_from_memory(&jpeg).unwrap().into_rgb8();
            let decoded = Command::new("ffmpeg")
                .args(["-hide_banner", "-loglevel", "error", "-ss", &format!("{seconds:.9}"), "-i"])
                .arg(&path)
                .args(["-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgb24", "pipe:1"])
                .output()
                .unwrap();
            assert!(decoded.status.success());
            assert_eq!(decoded.stdout.len(), preview.as_raw().len());
            let mut sum = 0u64;
            let mut count = 0u64;
            let mut bright = 0;
            for y in 780..1040 {
                for x in 480..1440 {
                    for c in 0..3 {
                        let i = (y * 1920 + x) * 3 + c;
                        sum += u64::from(preview.as_raw()[i].abs_diff(decoded.stdout[i]));
                        count += 1;
                        if decoded.stdout[i] > 170 {
                            bright += 1;
                        }
                    }
                }
            }
            assert!(bright > 100, "legenda ausente na exportação {mode}, quadro {number}");
            let error = sum as f64 / count as f64;
            mae = mae.max(error);
            comparisons.push(json!({"seconds":seconds,"mae":error}));
        }
        eprintln!("M1.1 {mode}: redraw_ms={redraws:?}; fps={fps:.3}; export_s={export_seconds:.3}; caption_mae={mae:.3}; stats={stats:?}");
        measurements.push(json!({"mode":mode,"redrawMs":redraws,"fps":fps,"exportSeconds":export_seconds,"mae":mae,"comparisons":comparisons,"stats":stats}));
    }
    let srt = output.join("minuto.srt");
    assert_eq!(request(&mut socket, 8, "captions.exportSrt", json!({"path":srt})).await["ok"], true);
    let bytes = std::fs::metadata(&srt).unwrap().len();
    let doc = filmcraft_captions::parse(&std::fs::read(&srt).unwrap(), filmcraft_captions::Format::Srt).unwrap();
    assert_eq!(doc.cues.len(), 20);
    eprintln!("M1.1 SRT 60s: {bytes} bytes; 20 frases UTF-8");
    std::fs::write(
        output.join("measurements.json"),
        serde_json::to_vec_pretty(&json!({"modes":measurements,"srtBytes":bytes,"encoder":encoder,"gpu":hello["result"]["gpu"]})).unwrap(),
    )
    .unwrap();
    request(&mut socket, 99, "shutdown", json!({})).await;
    motor.exited().await;
    assert!(measurements.iter().all(|m| m["mae"].as_f64().unwrap() < 8.0), "prévia/exportação diferentes");
    assert!(measurements.iter().all(|m| m["fps"].as_f64().unwrap() >= 29.5), "prévia abaixo de 30 qps");
    assert!(measurements.iter().all(|m| m["redrawMs"].as_array().unwrap().iter().all(|v| v.as_f64().unwrap() < 50.0)), "redesenho excede 50 ms");
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
    assert_eq!(hello["result"]["protocol"], 5);
    let fonts = hello["result"]["fonts"].as_array().unwrap();
    assert!(fonts.iter().any(|f| f == "Inter"));
    assert!(fonts.windows(2).all(|v| v[0].as_str().unwrap() < v[1].as_str().unwrap()));
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
    let mut socket = motor.connect().await;
    // ready confirma o socket; hello confirma que fontes e GPU já inicializaram.
    // O prazo abaixo mede o encerramento de uma sessão ativa, não o startup frio
    // do driver no runner Windows compartilhado.
    assert_eq!(request(&mut socket, 1, "hello", json!({})).await["ok"], true);
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
