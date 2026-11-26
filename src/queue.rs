use crate::catalog::TrackId;
use crate::playback::{PlaybackItem, QueueCommand};
use rand::seq::IteratorRandom;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    pub id: u64,
    pub item: PlaybackItem,
    pub suggested: bool,
}

#[derive(Default, PartialEq, Eq, Clone, Copy)]
pub(crate) enum Repeat {
    #[default]
    Off,
    Playlist,
    Track,
}
impl Repeat {
    pub fn cycle(self) -> Self {
        match self {
            Self::Off => Self::Playlist,
            Self::Playlist => Self::Track,
            Self::Track => Self::Off,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "None",
            Self::Playlist => "Playlist",
            Self::Track => "Track",
        }
    }
}

#[derive(Default)]
pub(crate) struct Queue {
    pub entries: Vec<Entry>,
    next_id: u64,
    pub selected: Option<u64>,
    pub seed: Option<PlaybackItem>,
    seen: Vec<TrackId>,
    history: Vec<u64>,
    failed: Vec<u64>,
    pub shuffle: bool,
    pub repeat: Repeat,
}
impl Queue {
    pub fn replace(&mut self, items: Vec<PlaybackItem>, index: usize) {
        self.seed = items.get(index).cloned();
        self.entries.clear();
        self.history.clear();
        self.failed.clear();
        self.seen.clear();
        for item in items {
            self.record_seen(&item.track_id);
            self.push(item, false);
        }
        self.selected = Some(self.entries[index].id);
    }
    fn record_seen(&mut self, track_id: &TrackId) {
        if !self.seen.contains(track_id) {
            self.seen.push(track_id.clone());
        }
    }
    fn push(&mut self, item: PlaybackItem, suggested: bool) {
        self.next_id += 1;
        self.entries.push(Entry {
            id: self.next_id,
            item,
            suggested,
        });
    }
    pub fn upcoming_count(&self) -> usize {
        let Some(index) = self.index() else {
            return 0;
        };
        self.entries[index + 1..]
            .iter()
            .filter(|e| !self.failed.contains(&e.id))
            .count()
    }
    pub fn seen_tracks(&self) -> Vec<TrackId> {
        let mut seen = self.seen.clone();
        for entry in &self.entries {
            if !seen.contains(&entry.item.track_id) {
                seen.push(entry.item.track_id.clone());
            }
        }
        seen
    }
    pub fn append_suggestions(&mut self, items: Vec<PlaybackItem>) -> usize {
        let mut added = 0;
        for item in items {
            if self.seen.contains(&item.track_id)
                || self
                    .entries
                    .iter()
                    .any(|e| e.item.track_id == item.track_id)
            {
                continue;
            }
            self.record_seen(&item.track_id);
            self.push(item, true);
            added += 1;
        }
        added
    }
    pub fn clear_upcoming_suggestions(&mut self) {
        let start = self.index().map_or(0, |i| i + 1);
        let mut idx = 0;
        let mut removed = Vec::new();
        self.entries.retain(|entry| {
            let keep = idx < start || !entry.suggested;
            if !keep {
                removed.push(entry.id);
            }
            idx += 1;
            keep
        });
        if !removed.is_empty() {
            self.history.retain(|id| !removed.contains(id));
            self.failed.retain(|id| !removed.contains(id));
        }
    }
    pub fn index(&self) -> Option<usize> {
        self.entries
            .iter()
            .position(|e| Some(e.id) == self.selected)
    }
    pub fn select(&mut self, id: u64) -> Result<(), String> {
        self.position(id)?;
        self.selected = Some(id);
        self.failed.clear();
        Ok(())
    }
    pub fn started(&mut self) {
        if let Some(id) = self.selected
            && self.history.last() != Some(&id)
        {
            self.history.push(id);
        }
    }
    pub fn previous(&mut self) -> Option<u64> {
        if self.shuffle {
            if self.history.last() == self.selected.as_ref() && self.history.len() > 1 {
                self.history.pop();
            }
            return self.history.last().copied().or(self.selected);
        }
        let index = self.index()?;
        let prev = if index == 0 && self.repeat == Repeat::Playlist {
            self.entries.len() - 1
        } else {
            index.saturating_sub(1)
        };
        Some(self.entries[prev].id)
    }
    pub fn next(&self, eof: bool) -> Option<u64> {
        let index = self.index()?;
        if eof && self.repeat == Repeat::Track && self.failed.is_empty() {
            return self.selected;
        }
        if self.shuffle && self.entries.len() > 1 {
            if let Some(next) = self
                .entries
                .iter()
                .filter(|e| {
                    Some(e.id) != self.selected
                        && !self.failed.contains(&e.id)
                        && !self.history.contains(&e.id)
                })
                .choose(&mut rand::rng())
            {
                return Some(next.id);
            }
            if self.repeat == Repeat::Off {
                return None;
            }
            return self
                .entries
                .iter()
                .filter(|e| Some(e.id) != self.selected && !self.failed.contains(&e.id))
                .choose(&mut rand::rng())
                .map(|e| e.id);
        }
        self.entries[index + 1..]
            .iter()
            .chain(self.entries[..=index].iter().filter(|_| {
                self.repeat == Repeat::Playlist
                    || (self.repeat == Repeat::Track && !self.failed.is_empty())
            }))
            .find(|e| !self.failed.contains(&e.id))
            .map(|e| e.id)
    }
    pub fn preview_next_index(&self) -> Option<usize> {
        if self.shuffle {
            return None;
        }
        let id = self.next(false)?;
        self.entries.iter().position(|entry| entry.id == id)
    }
    pub fn completed(&mut self) {
        self.failed.clear();
    }
    pub fn failed(&mut self) -> Option<u64> {
        self.failed.push(self.selected?);
        self.next(false)
    }
    fn position(&self, id: u64) -> Result<usize, String> {
        self.entries
            .iter()
            .position(|e| e.id == id)
            .ok_or_else(|| "A faixa não está mais na fila.".into())
    }
    pub fn edit(&mut self, command: QueueCommand) -> Result<(), String> {
        match command {
            QueueCommand::Enqueue(item) => {
                self.record_seen(&item.track_id);
                let insert_at = {
                    let start = self.index().map_or(0, |i| i + 1);
                    self.entries[start..]
                        .iter()
                        .position(|e| e.suggested)
                        .map(|rel| start + rel)
                };
                self.next_id += 1;
                let entry = Entry {
                    id: self.next_id,
                    item,
                    suggested: false,
                };
                if let Some(pos) = insert_at {
                    self.entries.insert(pos, entry);
                } else {
                    self.entries.push(entry);
                }
            }
            QueueCommand::Remove(id) => {
                let i = self.position(id)?;
                if self.selected == Some(id) {
                    return Err("A faixa atual não pode ser removida.".into());
                }
                self.entries.remove(i);
                self.history.retain(|v| *v != id);
                self.failed.retain(|v| *v != id);
            }
            QueueCommand::Move { from, to } => {
                let from = self.position(from)?;
                let to = self.position(to)?;
                if self.index().is_some_and(|i| from <= i || to <= i) {
                    return Err("Só é possível mover as próximas faixas.".into());
                }
                let e = self.entries.remove(from);
                self.entries.insert(to, e);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn duplicates() -> Queue {
        let item = PlaybackItem {
            track_id: "abc".to_owned().try_into().unwrap(),
            title: "A".into(),
            artist: String::new(),
            artwork_url: None,
            duration: 0,
        };
        let mut queue = Queue::default();
        queue.replace(vec![item; 4], 2);
        queue
    }

    #[test]
    fn history_follows_occurrences_across_queue_edits() {
        let mut q = duplicates();
        let ids = q.entries.iter().map(|e| e.id).collect::<Vec<_>>();
        q.shuffle = true;
        q.started();
        q.select(ids[0]).unwrap();
        q.started();
        q.edit(QueueCommand::Move {
            from: ids[2],
            to: ids[3],
        })
        .unwrap();
        q.edit(QueueCommand::Remove(ids[1])).unwrap();
        assert_eq!(q.previous(), Some(ids[2]));
        q.edit(QueueCommand::Remove(ids[2])).unwrap();
        assert_eq!(q.previous(), Some(ids[0]));
    }

    #[test]
    fn next_preview_follows_order_and_repeat_without_guessing_shuffle() {
        let mut queue = Queue::default();
        queue.replace(vec![item("a", "A"), item("b", "B")], 0);
        assert_eq!(queue.preview_next_index(), Some(1));

        queue.selected = Some(queue.entries[1].id);
        assert_eq!(queue.preview_next_index(), None);
        queue.repeat = Repeat::Playlist;
        assert_eq!(queue.preview_next_index(), Some(0));
        queue.shuffle = true;
        assert_eq!(queue.preview_next_index(), None);
    }

    #[test]
    fn failure_chain_visits_each_candidate_at_most_once() {
        for shuffle in [false, true] {
            for repeat in [Repeat::Off, Repeat::Track, Repeat::Playlist] {
                let mut q = duplicates();
                q.selected = Some(q.entries[0].id);
                q.shuffle = shuffle;
                q.repeat = repeat;
                let mut attempts = std::collections::HashSet::new();
                loop {
                    assert!(attempts.insert(q.selected.unwrap()));
                    let Some(next) = q.failed() else { break };
                    q.selected = Some(next);
                }
                assert_eq!(attempts.len(), 4);
            }
        }
    }

    fn item(id: &str, title: &str) -> PlaybackItem {
        PlaybackItem {
            track_id: id.to_owned().try_into().unwrap(),
            title: title.into(),
            artist: "Artista".into(),
            artwork_url: None,
            duration: 180_000_000,
        }
    }

    #[test]
    fn suggestions_respect_seed_deduplication_and_manual_queue_priority() {
        let mut q = Queue::default();
        let seed = item("yt:aaaaaaaaaaa", "Primeira Música");
        q.replace(vec![seed.clone()], 0);
        assert_eq!(q.seed.as_ref(), Some(&seed));
        assert_eq!(q.upcoming_count(), 0);

        let added = q.append_suggestions(vec![
            seed.clone(),
            item("yt:bbbbbbbbbbb", "Sugestão 1"),
            item("yt:bbbbbbbbbbb", "Sugestão 1 Duplicada"),
            item("yt:ccccccccccc", "Sugestão 2"),
        ]);
        assert_eq!(added, 2);
        assert_eq!(q.upcoming_count(), 2);
        assert!(!q.entries[0].suggested);
        assert!(q.entries[1].suggested);
        assert!(q.entries[2].suggested);

        // User manually enqueues a song: it goes before upcoming auto-suggestions
        q.edit(QueueCommand::Enqueue(item("yt:ddddddddddd", "Manual 1")))
            .unwrap();
        q.edit(QueueCommand::Enqueue(item("yt:eeeeeeeeeee", "Manual 2")))
            .unwrap();
        let titles: Vec<&str> = q.entries.iter().map(|e| e.item.title.as_str()).collect();
        assert_eq!(
            titles,
            vec![
                "Primeira Música",
                "Manual 1",
                "Manual 2",
                "Sugestão 1",
                "Sugestão 2",
            ]
        );
        assert!(!q.entries[1].suggested);
        assert!(!q.entries[2].suggested);
        assert!(q.entries[3].suggested);

        // Clearing upcoming suggestions preserves current track and manual tracks
        q.clear_upcoming_suggestions();
        let remaining: Vec<&str> = q.entries.iter().map(|e| e.item.title.as_str()).collect();
        assert_eq!(remaining, vec!["Primeira Música", "Manual 1", "Manual 2"]);

        // Seen tracks still remembers removed suggestions so they aren't repeated
        let seen = q.seen_tracks();
        assert!(seen.contains(&"yt:bbbbbbbbbbb".to_owned().try_into().unwrap()));
        assert!(seen.contains(&"yt:ccccccccccc".to_owned().try_into().unwrap()));
    }
}
