//! Snapshot da mistura, com decoder independente e tempos da linha do tempo.
use crate::protocol::{Result, error};
use filmcraft_engine::{MediaPool, Services, Session};
use filmcraft_project::{ItemId, Project};
use filmcraft_time::Tick;
use std::sync::Arc;

#[derive(Clone)]
pub struct Snapshot {
    project: Arc<Project>,
    sequence: ItemId,
    services: Arc<dyn Services>,
    pub duration: Tick,
    origin: Tick,
}
impl Snapshot {
    pub fn new(session: &Session) -> Result<Self> {
        let sequence = session.state.active_sequence.ok_or_else(|| error("no_session", "Abra uma sessão antes de calcular o áudio."))?;
        let duration = session.active_sequence().ok_or_else(|| error("no_session", "Abra uma sessão antes de calcular o áudio."))?.duration();
        Ok(Self { project: session.project.clone(), sequence, services: session.services.clone(), duration, origin: Tick::ZERO })
    }
    pub fn clip(session: &Session, ids: &[filmcraft_project::ClipId], origin: Tick, duration: Tick) -> Result<Self> {
        let mut snapshot = Self::new(session)?;
        let project = Arc::make_mut(&mut snapshot.project);
        if let Some(sequence) = project.sequence_mut(snapshot.sequence) {
            for track in sequence.audio_tracks.iter_mut() {
                track.items.retain(|item| ids.contains(&item.id));
                track.transitions.clear();
            }
        }
        snapshot.origin = origin;
        snapshot.duration = duration;
        Ok(snapshot)
    }
    pub fn reader(&self, rate: u32) -> impl Fn(i64, usize) -> Vec<Vec<f32>> {
        let mut project = (*self.project).clone();
        if let Some(sequence) = project.sequence_mut(self.sequence) {
            sequence.settings.sample_rate = rate;
        }
        let project = Arc::new(project);
        let sources = Arc::new(MediaPool::default()).provider(project.clone(), self.services.clone());
        let sequence = self.sequence;
        let origin = self.origin.to_units_floor(rate as i64);
        move |start, count| {
            filmcraft_render::audio::mix_sequence(&project, project.sequence(sequence).expect("sequência do snapshot"), origin + start, count, &sources)
                .channels
        }
    }
}
