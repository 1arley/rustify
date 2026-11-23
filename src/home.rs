use crate::design::{ACCENT, ERROR, INFO, MUTED};
use crate::playback::PlaybackState;

pub(crate) fn playlist_duration(tracks: &[crate::catalog::Track]) -> String {
    if tracks.iter().any(|track| track.duration_micros() == 0) {
        return "Duração indisponível".to_owned();
    }
    let total = tracks.iter().fold(0_i64, |total, track| {
        total.saturating_add(track.duration_micros())
    });
    if total == 0 {
        return "Duração indisponível".to_owned();
    }
    let minutes = total / 60_000_000;
    if minutes >= 60 {
        format!("{} h {} min", minutes / 60, minutes % 60)
    } else if minutes > 0 {
        format!("{minutes} min")
    } else {
        "Menos de 1 min".to_owned()
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Track;

    #[test]
    fn playlist_duration_handles_unknown_short_and_long_playlists() {
        let track = |seconds| {
            Track::from_youtube(
                "abcdefghijk".into(),
                "Faixa".into(),
                "Artista".into(),
                seconds,
                None,
            )
        };
        assert_eq!(playlist_duration(&[]), "Duração indisponível");
        assert_eq!(playlist_duration(&[track(0.0)]), "Duração indisponível");
        assert_eq!(
            playlist_duration(&[track(180.0), track(0.0)]),
            "Duração indisponível"
        );
        assert_eq!(
            playlist_duration(&[track(0.0), track(180.0)]),
            "Duração indisponível"
        );
        assert_eq!(playlist_duration(&[track(30.0)]), "Menos de 1 min");
        assert_eq!(playlist_duration(&[track(180.0), track(120.0)]), "5 min");
        assert_eq!(
            playlist_duration(&[track(3600.0), track(120.0)]),
            "1 h 2 min"
        );
    }
}
