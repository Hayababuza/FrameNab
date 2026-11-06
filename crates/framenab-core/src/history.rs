//! Undo/Redo über Snapshots. Ein [`crate::Project`] enthält keine Pixeldaten (die liegen als
//! Dateien im Projektordner), daher sind Snapshots klein und dieser Ansatz ist robust und einfach.

#[derive(Debug, Clone)]
pub struct History<T> {
    undo: Vec<T>,
    redo: Vec<T>,
    limit: usize,
}

impl<T: Clone> History<T> {
    #[must_use]
    pub fn new(limit: usize) -> Self {
        Self { undo: Vec::new(), redo: Vec::new(), limit: limit.max(1) }
    }

    /// Merkt sich den Zustand *vor* einer Änderung.
    pub fn record(&mut self, before: &T) {
        self.undo.push(before.clone());
        if self.undo.len() > self.limit {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Macht die letzte Änderung rückgängig und gibt den wiederhergestellten Zustand zurück.
    pub fn undo(&mut self, current: &T) -> Option<T> {
        let prev = self.undo.pop()?;
        self.redo.push(current.clone());
        Some(prev)
    }

    pub fn redo(&mut self, current: &T) -> Option<T> {
        let next = self.redo.pop()?;
        self.undo.push(current.clone());
        Some(next)
    }

    #[must_use]
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    #[must_use]
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::History;

    #[test]
    fn undo_redo_cycle() {
        let mut h = History::new(10);
        let mut state = 1;
        h.record(&state);
        state = 2;
        h.record(&state);
        state = 3;

        state = h.undo(&state).unwrap();
        assert_eq!(state, 2);
        state = h.undo(&state).unwrap();
        assert_eq!(state, 1);
        assert!(h.undo(&state).is_none());
        state = h.redo(&state).unwrap();
        assert_eq!(state, 2);

        h.record(&state); // neue Änderung verwirft Redo
        assert!(!h.can_redo());
    }

    #[test]
    fn limit_drops_oldest() {
        let mut h = History::new(2);
        for i in 0..5 {
            h.record(&i);
        }
        assert_eq!(h.undo(&9), Some(4));
        assert_eq!(h.undo(&4), Some(3));
        assert_eq!(h.undo(&3), None);
    }
}
