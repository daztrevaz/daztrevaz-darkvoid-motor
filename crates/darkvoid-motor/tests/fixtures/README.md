# Mídia sintética dos testes

`padrao.mp4` contém três segundos de padrão de cor (64 × 36, 30 fps, H.264) e tom de
440 Hz (AAC, 48 kHz). Não contém material de terceiros ou dados de clientes.

Gerado localmente com FFmpeg, sem usar FFmpeg no produto nem nos testes:

```powershell
ffmpeg -hide_banner -loglevel error -f lavfi -i testsrc2=size=64x36:rate=30 -f lavfi -i sine=frequency=440:sample_rate=48000 -t 3 -c:v libx264 -pix_fmt yuv420p -g 30 -bf 0 -c:a aac -b:a 64k -y padrao.mp4
```

O teste de reprodução física é ignorado no CI. Para executá-lo com o dispositivo
padrão de áudio conectado:

```powershell
cargo test -p darkvoid-motor --test protocol playback_audio_clock_pause_resume_seek_and_end -- --ignored --nocapture
```
