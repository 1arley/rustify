use crate::design::{ACCENT, ERROR, INFO, MUTED};
use crate::playback::PlaybackState;

pub(crate) fn playback_status(state: &PlaybackState) -> (&'static str, u32) {
    if state.loading {
        ("CARREGANDO", INFO)
    } else if state.retry {
        ("FALHA NA REPRODUÇÃO", ERROR)
    } else if state.playing {
        ("TOCANDO AGORA", ACCENT)
    } else {
        ("PLAYER", MUTED)
    }
}
