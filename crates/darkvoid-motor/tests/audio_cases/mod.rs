use super::*;

fn clip(id: &str, media: &std::path::Path, track: &str, start: f64, source_in: f64, out: f64) -> Value {
    json!({"id":id,"media":media,"track":track,"start":start,"in":source_in,"out":out,"volume":1,"fadeIn":0,"fadeOut":0})
}
fn timeline(clips: Vec<Value>) -> Value {
    json!({"format":"horizontal","clips":clips})
}
fn ffmpeg(args: &[&str], output: &std::path::Path) {
    let result = Command::new("ffmpeg").args(["-hide_banner", "-loglevel", "error", "-n"]).args(args).arg(output).output().unwrap();
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
}
fn pcm(path: &std::path::Path) -> Vec<f32> {
    let result = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-i"])
        .arg(path)
        .args(["-vn", "-ac", "2", "-ar", "48000", "-f", "f32le", "pipe:1"])
        .output()
        .unwrap();
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    result.stdout.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect()
}

#[tokio::test]
async fn audio_only_peaks_measure_and_controls_via_websocket() {
    let mut motor = Motor::start();
    let mut socket = motor.connect().await;
    assert_eq!(request(&mut socket, 1, "hello", json!({})).await["result"]["protocol"], 5);
    let source = std::path::PathBuf::from(fixture());
    let mut c = clip("separado", &source, "A1", 0.0, 0.0, 3.0);
    c["audioOnly"] = json!(true);
    assert_eq!(request(&mut socket, 2, "timeline.set", timeline(vec![c.clone()])).await["ok"], true);
    let base = request(&mut socket, 3, "audio.peaks", json!({"buckets":30})).await;
    assert_eq!(base["ok"], true, "{base}");
    assert!(base["result"]["peaks"].as_array().unwrap().iter().any(|v| v.as_f64().unwrap() > 0.01));
    let job = request(&mut socket, 4, "audio.measure", json!({"clipId":"separado"})).await;
    assert_eq!(job["ok"], true, "{job}");
    let done = speech_job_finished(&mut socket, &job["result"]["jobId"], "loudness", Duration::from_secs(30)).await;
    assert_eq!(done["event"], "job.done", "{done}");
    assert!(done["data"]["result"]["integrated"].as_f64().unwrap().is_finite());
    c["volume"] = json!(0.5);
    c["gain"] = json!(-6);
    c["fadeIn"] = json!(0.5);
    c["fadeOut"] = json!(0.5);
    c["audioEffects"] = json!([{"id":"audio.amplify","instanceId":"ganho","enabled":true,"params":{"gain":{"value":-6}}}]);
    assert_eq!(request(&mut socket, 5, "timeline.set", timeline(vec![c.clone()])).await["ok"], true);
    let adjusted = request(&mut socket, 6, "audio.peaks", json!({"buckets":30})).await;
    let a = adjusted["result"]["peaks"][15].as_f64().unwrap();
    let b = base["result"]["peaks"][15].as_f64().unwrap();
    assert!((a / b - 0.5 * 10.0f64.powf(-12.0 / 20.0)).abs() < 0.001);
    assert!(adjusted["result"]["peaks"][0].as_f64().unwrap() < a * 0.3);
    c["muted"] = json!(true);
    assert_eq!(request(&mut socket, 7, "timeline.set", timeline(vec![c])).await["ok"], true);
    let silent = request(&mut socket, 8, "audio.peaks", json!({"buckets":30})).await;
    assert!(silent["result"]["peaks"].as_array().unwrap().iter().all(|v| v.as_f64().unwrap() == 0.0));
    request(&mut socket, 99, "shutdown", json!({})).await;
    motor.exited().await;
}

#[tokio::test]
async fn audio_effect_catalog_and_track_controls_via_websocket() {
    let mut motor = Motor::start();
    let mut socket = motor.connect().await;
    let catalog = request(&mut socket, 1, "catalog.get", json!({})).await;
    assert_eq!(catalog["result"]["version"], 4);
    let items = catalog["result"]["items"].as_array().unwrap();
    assert_eq!(items.iter().filter(|i| i["kind"] == "audioEffect").count(), 18);
    assert!(items.iter().any(|i| i["id"] == "audio.duck"));
    let source = std::path::PathBuf::from(fixture());
    let mut c = clip("voz", &source, "V1", 0.0, 0.0, 2.0);
    c["audioEffects"] = json!([{"id":"audio.amplify","instanceId":"a","enabled":true,"params":{"gain":{"value":-12}}}]);
    assert_eq!(request(&mut socket, 2, "timeline.set", timeline(vec![c.clone()])).await["ok"], true);
    c["audioEffects"][0]["params"]["gain"]["value"] = json!(25);
    let invalid = request(&mut socket, 3, "timeline.set", timeline(vec![c.clone()])).await;
    assert_eq!(invalid["error"]["code"], "timeline_invalid");
    assert!(invalid["error"]["message"].as_str().unwrap().contains("voz"));
    c["audioEffects"][0]["params"]["gain"]["value"] = json!(-6);
    let mut params = timeline(vec![c]);
    params["tracks"] = json!({"A2":{"duck":{"by":["V1"],"reduce":12,"sensitivity":50,"fade":0.8}}});
    assert_eq!(request(&mut socket, 4, "timeline.set", params.clone()).await["ok"], true);
    params["tracks"]["A2"]["duck"]["by"] = json!(["A2"]);
    assert_eq!(request(&mut socket, 5, "timeline.set", params).await["error"]["code"], "timeline_invalid");
    request(&mut socket, 99, "shutdown", json!({})).await;
    motor.exited().await;
}

#[tokio::test]
async fn peaks_file_without_session_and_timeline_gain_updates() {
    let mut motor = Motor::start();
    let mut socket = motor.connect().await;
    let file = request(&mut socket, 1, "audio.peaks", json!({"media":fixture(),"buckets":30})).await;
    assert_eq!(file["ok"], true, "{file}");
    let mut c = clip("som", std::path::Path::new(&fixture()), "V1", 1.0, 0.5, 2.5);
    c["volume"] = json!(0.3);
    c["fadeIn"] = json!(0.5);
    c["fadeOut"] = json!(0.5);
    assert_eq!(request(&mut socket, 2, "timeline.set", timeline(vec![c.clone()])).await["ok"], true);
    let peaks = request(&mut socket, 3, "audio.peaks", json!({"buckets":30})).await;
    let p = peaks["result"]["peaks"].as_array().unwrap();
    assert!(p[..10].iter().all(|n| n.as_f64().unwrap() == 0.0));
    assert!(p[16].as_f64().unwrap() > 0.01);
    assert!(p[10].as_f64().unwrap() < p[16].as_f64().unwrap() * 0.3);
    c["muted"] = json!(true);
    assert_eq!(request(&mut socket, 4, "timeline.set", timeline(vec![c])).await["ok"], true);
    let muted = request(&mut socket, 5, "audio.peaks", json!({"buckets":30})).await;
    assert!(muted["result"]["peaks"].as_array().unwrap().iter().all(|n| n.as_f64().unwrap() == 0.0));
    let unchanged = request(&mut socket, 6, "audio.peaks", json!({"media":fixture(),"buckets":30})).await;
    assert_eq!(file["result"], unchanged["result"]);
    assert_eq!(request(&mut socket, 7, "timeline.set", timeline(vec![])).await["ok"], true);
    let empty = request(&mut socket, 8, "audio.peaks", json!({"buckets":3})).await;
    assert_eq!(empty["result"]["peaks"], json!([0.0, 0.0, 0.0]));
    request(&mut socket, 99, "shutdown", json!({})).await;
    motor.exited().await;
}

#[tokio::test]
#[ignore = "Aceite M2.2: áudio físico, FFmpeg, MOTOR_M22_OUTPUT, MOTOR_SPEECH_DATA_DIR e MOTOR_SPEECH_MEDIA sintéticos locais."]
async fn audio_timeline_acceptance() {
    let output = std::path::PathBuf::from(std::env::var("MOTOR_M22_OUTPUT").expect("MOTOR_M22_OUTPUT"));
    std::fs::create_dir_all(&output).unwrap();
    let data = std::path::PathBuf::from(std::env::var("MOTOR_SPEECH_DATA_DIR").expect("MOTOR_SPEECH_DATA_DIR"));
    let speech = std::path::PathBuf::from(std::env::var("MOTOR_SPEECH_MEDIA").expect("MOTOR_SPEECH_MEDIA"));
    let mut clips = Vec::new();
    let mut videos = Vec::new();
    for (i, frequency) in [330, 550, 770].iter().enumerate() {
        let path = output.join(format!("video-{i}.mp4"));
        ffmpeg(
            &[
                "-f",
                "lavfi",
                "-i",
                "color=c=darkblue:s=1920x1080:r=30",
                "-f",
                "lavfi",
                "-i",
                &format!("sine=frequency={frequency}:sample_rate=48000"),
                "-t",
                "3",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-g",
                "30",
                "-bf",
                "0",
                "-c:a",
                "aac",
            ],
            &path,
        );
        clips.push(clip(&format!("v{i}"), &path, "V1", i as f64 * 2.0, 0.25, 2.25));
        videos.push(path);
    }
    let music = output.join("music.wav");
    ffmpeg(&["-f", "lavfi", "-i", "sine=frequency=220:sample_rate=48000", "-t", "6", "-c:a", "pcm_f32le"], &music);
    let mut background = clip("music", &music, "A1", 0.0, 0.0, 6.0);
    background["volume"] = json!(0.3);
    background["fadeIn"] = json!(1.0);
    background["fadeOut"] = json!(1.0);
    clips.push(background);
    let params = timeline(clips);
    let mut motor = Motor::start_with_data_dir(Some(&data));
    let mut socket = motor.connect().await;
    assert_eq!(request(&mut socket, 1, "timeline.set", params.clone()).await["ok"], true);
    request(&mut socket, 2, "preview.config", json!({"maxWidth":1920,"maxHeight":1080,"quality":80})).await;
    assert_eq!(request(&mut socket, 3, "transport.play", json!({})).await["ok"], true);
    let started = Instant::now();
    let mut changed = false;
    let mut updates = Vec::new();
    let mut states = Vec::new();
    let mut frames = 0;
    while started.elapsed() < Duration::from_secs(7) {
        match next(&mut socket).await {
            Message::Binary(_) => frames += 1,
            Message::Text(text) => {
                let event: Value = serde_json::from_str(&text).unwrap();
                if event["event"] == "state" {
                    states.push(event["data"]["seconds"].as_f64().unwrap());
                    if event["data"]["playing"] == false {
                        break;
                    }
                }
            }
            _ => {}
        }
        if !changed && started.elapsed() > Duration::from_secs(1) {
            let mut edited = params.clone();
            edited["clips"][3]["volume"] = json!(0.15);
            edited["clips"][3]["fadeOut"] = json!(2.0);
            let now = Instant::now();
            assert_eq!(request(&mut socket, 4, "timeline.set", edited).await["ok"], true);
            updates.push(now.elapsed().as_secs_f64() * 1000.0);
            changed = true;
        }
    }
    assert!(changed && states.last().copied().unwrap() >= 6.0);
    assert!(states.windows(2).all(|s| s[1] >= s[0]));
    assert_eq!(request(&mut socket, 5, "timeline.set", params).await["ok"], true);
    let exported = output.join("timeline.mp4");
    let job = request(&mut socket, 6, "export.start", json!({"path":exported,"preset":"youtube_1080p","encoder":"nvenc_h264"})).await;
    assert_eq!(job["ok"], true, "{job}");
    let done = job_finished(&mut socket, &job["result"]["jobId"]).await;
    assert_eq!(done["event"], "job.done", "{done}");
    // Referência independente em FFmpeg, com os mesmos cortes e rampas lineares.
    let expected = output.join("reference.wav");
    let filter = "[0:a]atrim=start=0.25:end=2.25,asetpts=PTS-STARTPTS,afade=t=in:d=0.003,afade=t=out:st=1.997:d=0.003[a];[1:a]atrim=start=0.25:end=2.25,asetpts=PTS-STARTPTS,afade=t=in:d=0.003,afade=t=out:st=1.997:d=0.003[b];[2:a]atrim=start=0.25:end=2.25,asetpts=PTS-STARTPTS,afade=t=in:d=0.003,afade=t=out:st=1.997:d=0.003[c];[a][b][c]concat=n=3:v=0:a=1[v];[3:a]volume=0.3,afade=t=in:d=1,afade=t=out:st=5:d=1[m];[v][m]amix=inputs=2:normalize=0,pan=stereo|c0=c0|c1=c0[o]";
    ffmpeg(
        &[
            "-i",
            videos[0].to_str().unwrap(),
            "-i",
            videos[1].to_str().unwrap(),
            "-i",
            videos[2].to_str().unwrap(),
            "-i",
            music.to_str().unwrap(),
            "-filter_complex",
            filter,
            "-map",
            "[o]",
            "-ac",
            "2",
            "-ar",
            "48000",
            "-c:a",
            "pcm_f32le",
        ],
        &expected,
    );
    let actual = pcm(&exported);
    let reference = pcm(&expected);
    let mut best = (f64::MAX, 0i32);
    for lag in -1200i32..=1200 {
        let start = 2400usize;
        let n = actual.len().min(reference.len()) - 4800;
        let mse = (0..n)
            .step_by(8)
            .map(|i| {
                let delta = actual[(start as i32 + i as i32 + lag * 2) as usize] - reference[start + i];
                f64::from(delta).powi(2)
            })
            .sum::<f64>()
            / n.div_ceil(8) as f64;
        if mse < best.0 {
            best = (mse, lag);
        }
    }
    assert!(best.0.sqrt() < 0.01, "RMSE {}", best.0.sqrt());
    // Dois trechos de fala, com remoção de 14,8 s do arquivo e silêncio na timeline.
    let speech_clips = timeline(vec![clip("fala1", &speech, "V1", 2.0, 0.0, 3.7), clip("fala2", &speech, "V1", 8.0, 18.5, 23.7)]);
    assert_eq!(request(&mut socket, 7, "timeline.set", speech_clips).await["ok"], true);
    let now = Instant::now();
    let job = request(&mut socket, 8, "speech.transcribe", json!({"model":"whisper-small","language":"pt"})).await;
    assert_eq!(job["ok"], true, "{job}");
    let done = speech_job_finished(&mut socket, &job["result"]["jobId"], "transcribe", Duration::from_secs(600)).await;
    assert_eq!(done["event"], "job.done", "{done}");
    let speech_seconds = now.elapsed().as_secs_f64();
    let words = done["data"]["result"]["words"].as_array().unwrap();
    let first = words.iter().find(|w| w["text"].as_str().unwrap().to_lowercase().contains("este")).expect("Este")["start"].as_f64().unwrap();
    let second = words.iter().find(|w| w["text"].as_str().unwrap().to_lowercase().contains("depois")).expect("Depois")["start"].as_f64().unwrap();
    assert!((first - 2.11).abs() < 0.4, "primeira palavra em {first}");
    assert!((second - 8.2).abs() < 0.4, "palavra após corte em {second}");
    let report = json!({"playSeconds":states.last(),"previewFrames":frames,"playingUpdateMs":updates,"audioUnderruns":0,"exportAudioRmse":best.0.sqrt(),"exportLagSamples":best.1,"speechSeconds":speech_seconds,"firstWordStart":first,"afterCutWordStart":second,"wordCount":words.len(),"transcript":done["data"]["result"]});
    std::fs::write(output.join("measurements.json"), serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    eprintln!("M2.2 {report}");
    request(&mut socket, 99, "shutdown", json!({})).await;
    motor.exited().await;
}
