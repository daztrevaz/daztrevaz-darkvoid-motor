//! Adaptadores seguros para os codificadores e o compositor já existentes no engine.
use filmcraft_export::{ExportSettings, Format, FrameRenderer, HardwareEncoding};
use filmcraft_project::{ItemId, Project};
use filmcraft_render::{Image, RenderOptions, SourceProvider};
use filmcraft_time::Tick;
use serde::Deserialize;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
pub enum Encoder {
    #[serde(rename = "software")]
    Software,
    #[serde(rename = "nvenc_h264")]
    NvencH264,
    #[serde(rename = "nvenc_hevc")]
    NvencHevc,
}

#[derive(Default)]
pub struct Capabilities {
    pub h264: bool,
    pub hevc: bool,
}
impl Capabilities {
    pub fn detect() -> Self {
        filmcraft_export::register_frame_renderer(gpu_factory);
        #[cfg(target_os = "windows")]
        {
            use filmcraft_platform::nvenc;
            filmcraft_export::register_encoder(nvenc::export::factory);
            filmcraft_export::register_format_probe(Format::Hevc, nvenc::hevc_available);
            let settings = ExportSettings { hardware_encoding: HardwareEncoding::Auto, ..Default::default() };
            // Abrir e inicializar cada codec realmente, em vez de confiar no nome da GPU.
            let probe = |format| {
                nvenc::export::factory(format, 640, 360, filmcraft_time::FrameRate::FPS_30, &settings)
                    .is_some_and(|result| result.is_ok_and(|encoder| encoder.hardware() == Some("NVENC")))
            };
            Self { h264: probe(Format::H264), hevc: probe(Format::Hevc) }
        }
        #[cfg(not(target_os = "windows"))]
        Self::default()
    }
    pub fn names(&self) -> Vec<&'static str> {
        let mut names = vec!["software"];
        if self.h264 {
            names.push("nvenc_h264");
        }
        if self.hevc {
            names.push("nvenc_hevc");
        }
        names
    }
    pub fn apply(&self, requested: Encoder, settings: &mut ExportSettings) {
        let supported = match requested {
            Encoder::Software => false,
            Encoder::NvencH264 => self.h264,
            Encoder::NvencHevc => self.hevc,
        };
        settings.format = if supported && requested == Encoder::NvencHevc { Format::Hevc } else { Format::H264 };
        settings.hardware_encoding = if supported { HardwareEncoding::Auto } else { HardwareEncoding::Off };
    }
}

struct Gpu(filmcraft_gpu::ExportRenderer);
impl FrameRenderer for Gpu {
    fn render(&mut self, project: &Project, seq: ItemId, time: Tick, opts: RenderOptions, sources: &dyn SourceProvider) -> Option<Image> {
        self.0.render(project, seq, time, opts, sources)
    }
    fn caps(&self) -> filmcraft_render::plan::DeviceCaps {
        self.0.caps()
    }
}
fn gpu_factory() -> Option<Box<dyn FrameRenderer>> {
    filmcraft_gpu::ExportRenderer::new().map(|renderer| Box::new(Gpu(renderer)) as Box<dyn FrameRenderer>)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unavailable_nvenc_falls_back_to_h264_software() {
        for encoder in [Encoder::Software, Encoder::NvencH264, Encoder::NvencHevc] {
            let mut settings = ExportSettings::default();
            Capabilities::default().apply(encoder, &mut settings);
            assert_eq!(settings.format, Format::H264);
            assert_eq!(settings.hardware_encoding, HardwareEncoding::Off);
        }
        assert_eq!(Capabilities::default().names(), ["software"]);
    }
}
