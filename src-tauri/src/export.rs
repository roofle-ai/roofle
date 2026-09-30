//! Transcript export: rendering and the exports folder.

use std::path::PathBuf;

use chrono::Local;
use roofle_core::storage::AudioSource;

use crate::models::ModelManager;
use crate::state::TranscriptCue;

const FALLBACK_ROOT: &str = "assets";
const LAST_CUE_SECS: f64 = 2.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    Txt,
    Srt,
}

impl ExportFormat {
    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Txt => "txt",
            ExportFormat::Srt => "srt",
        }
    }

    pub fn render(self, cues: &[TranscriptCue]) -> String {
        match self {
            ExportFormat::Txt => render_txt(cues),
            ExportFormat::Srt => render_srt(cues),
        }
    }
}

/// `<models_root>/../exports`, created on demand.
pub fn exports_dir(models: &ModelManager) -> Result<PathBuf, String> {
    let dir = models
        .root()
        .parent()
        .unwrap_or_else(|| std::path::Path::new(FALLBACK_ROOT))
        .join("exports");
    std::fs::create_dir_all(&dir).map_err(|e| format!("create exports dir: {e}"))?;
    Ok(dir)
}

pub fn write_export(models: &ModelManager, filename: &str, content: &str) -> Result<String, String> {
    let path = exports_dir(models)?.join(filename);
    std::fs::write(&path, content).map_err(|e| format!("write export: {e}"))?;
    Ok(path.display().to_string())
}

/// `YYYYMMDD-HHMMSS` in local time.
pub fn filename_stamp() -> String {
    Local::now().format("%Y%m%d-%H%M%S").to_string()
}

fn render_txt(cues: &[TranscriptCue]) -> String {
    cues.iter()
        .map(|cue| {
            let tag = match cue.source {
                AudioSource::Mic => "[Mic]",
                AudioSource::System => "[System]",
            };
            format!("{tag} {}\n", cue.text.trim())
        })
        .collect()
}

/// Each cue spans from its start to the next cue's start.
fn render_srt(cues: &[TranscriptCue]) -> String {
    let mut out = String::new();
    for (i, cue) in cues.iter().enumerate() {
        let end = cues
            .get(i + 1)
            .map_or(cue.start_secs + LAST_CUE_SECS, |next| next.start_secs);
        out.push_str(&format!(
            "{}\n{} --> {}\n{}\n\n",
            i + 1,
            srt_timestamp(cue.start_secs),
            srt_timestamp(end),
            cue.text.trim()
        ));
    }
    out
}

fn srt_timestamp(secs: f64) -> String {
    let total_ms = (secs.max(0.0) * 1000.0) as u64;
    let ms = total_ms % 1000;
    let total_s = total_ms / 1000;
    format!(
        "{:02}:{:02}:{:02},{ms:03}",
        total_s / 3600,
        (total_s / 60) % 60,
        total_s % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cue(secs: f64, text: &str) -> TranscriptCue {
        TranscriptCue {
            start_secs: secs,
            source: AudioSource::Mic,
            text: text.to_string(),
        }
    }

    #[test]
    fn export_format_parses_lowercase() {
        let txt: ExportFormat = serde_json::from_str("\"txt\"").unwrap();
        let srt: ExportFormat = serde_json::from_str("\"srt\"").unwrap();
        assert_eq!(txt, ExportFormat::Txt);
        assert_eq!(srt, ExportFormat::Srt);
    }

    #[test]
    fn txt_renders_one_line_per_cue() {
        let out = render_txt(&[cue(1.0, "hello world"), cue(4.0, "second line")]);
        assert_eq!(out, "[Mic] hello world\n[Mic] second line\n");
    }

    #[test]
    fn txt_labels_utterances_with_their_source() {
        let mut system = cue(1.0, "speaker audio");
        system.source = AudioSource::System;
        let out = render_txt(&[cue(1.0, "user speech"), system]);
        assert_eq!(out, "[Mic] user speech\n[System] speaker audio\n");
    }

    #[test]
    fn srt_renders_timestamped_cues() {
        let out = render_srt(&[cue(0.0, "first"), cue(3.5, "second")]);
        assert!(out.contains("00:00:00,000 --> 00:00:03,500"), "got: {out}");
        assert!(out.contains("00:00:03,500 --> 00:00:05,500"), "got: {out}");
        assert!(out.starts_with("1\n"), "first cue numbered 1, got: {out}");
        assert!(out.contains("\n2\n"), "second cue numbered 2, got: {out}");
    }
}
