use crate::playback::{PlaybackItem, QueueCommand};
use rand::seq::IteratorRandom;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    pub id: u64,
    pub item: PlaybackItem,
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
    history: Vec<u64>,
    failed: Vec<u64>,
    pub shuffle: bool,
    pub repeat: Repeat,
}
impl Queue {
    pub fn replace(&mut self, items: Vec<PlaybackItem>, index: usize) {
        self.entries.clear();
        self.history.clear();
        self.failed.clear();
        for item in items {
            self.push(item);
        }
        self.selected = Some(self.entries[index].id);
    }
    fn push(&mut self, item: PlaybackItem) {
        self.next_id += 1;
        self.entries.push(Entry {
            id: self.next_id,
            item,
        });
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
            QueueCommand::Enqueue(item) => self.push(item),
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
}
