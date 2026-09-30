use roofle_core::pipeline::{
    BothSource, MicSource, Pipeline, PipelineConfig, Source, DEFAULT_RING_CAPACITY,
};
#[cfg(target_os = "macos")]
use roofle_core::pipeline::SystemSource;
use tauri::{AppHandle, Manager};

use super::control::SourceKind;
use crate::state::AppState;

const DEFAULT_GAIN: f32 = 1.0;

/// Build and start a pipeline, downloading the selected model first if needed.
pub fn start(app: &AppHandle, kind: SourceKind) -> Result<Pipeline, String> {
    let state = app.state::<AppState>();
    let model_id = state.selected_model.lock().unwrap().clone();
    let model_dir = state
        .models
        .ensure(app, &model_id)
        .map_err(|e| format!("model {model_id}: {e}"))?;
    let vad_model = state.models.ensure_vad(app)?;

    let ring_capacity = match kind {
        SourceKind::Both => DEFAULT_RING_CAPACITY * 2,
        _ => DEFAULT_RING_CAPACITY,
    };

    let mut pipeline = Pipeline::new(PipelineConfig {
        model_dir,
        source: build_source(kind)?,
        ring_capacity: Some(ring_capacity),
        vad_model: Some(vad_model),
    })
    .map_err(|e| e.to_string())?;
    pipeline.start().map_err(|e| e.to_string())?;
    Ok(pipeline)
}

/// Stop the pipeline, flushing trailing speech.
pub fn stop(mut pipeline: Pipeline) {
    pipeline.stop();
}

#[cfg(target_os = "macos")]
fn build_source(kind: SourceKind) -> Result<Box<dyn Source>, String> {
    Ok(match kind {
        SourceKind::Mic => Box::new(MicSource::new(None, DEFAULT_GAIN)),
        SourceKind::System => Box::new(SystemSource::new(DEFAULT_GAIN)),
        SourceKind::Both => Box::new(BothSource::new(DEFAULT_GAIN)),
    })
}

#[cfg(not(target_os = "macos"))]
fn build_source(kind: SourceKind) -> Result<Box<dyn Source>, String> {
    match kind {
        SourceKind::Mic => Ok(Box::new(MicSource::new(None, DEFAULT_GAIN))),
        SourceKind::System | SourceKind::Both => {
            Err("system audio is not implemented on this platform".to_string())
        }
    }
}
