//! Выделение текста (SPEC §8.1).

use std::ops::Range;

/// Выделение: anchor закреплён, head — «текущий» конец (куда движется курсор).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor: usize,
    pub head: usize,
}

impl Selection {
    pub fn collapsed(offset: usize) -> Self {
        Self {
            anchor: offset,
            head: offset,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    /// Диапазон выделения (start <= end).
    pub fn range(&self) -> Range<usize> {
        if self.anchor <= self.head {
            self.anchor..self.head
        } else {
            self.head..self.anchor
        }
    }

    /// Свернуть к голове.
    pub fn collapse_to_head(&mut self) {
        self.anchor = self.head;
    }

    /// Свернуть к точке.
    pub fn collapse_to(&mut self, offset: usize) {
        self.anchor = offset;
        self.head = offset;
    }

    /// Переместить голову (и anchor, если выделение не расширяется).
    pub fn move_to(&mut self, offset: usize, extend: bool) {
        self.head = offset;
        if !extend {
            self.anchor = offset;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_normalizes_reversed() {
        let selection = Selection {
            anchor: 10,
            head: 4,
        };
        assert_eq!(selection.range(), 4..10);
        assert!(!selection.is_empty());
    }

    #[test]
    fn move_extend() {
        let mut selection = Selection::collapsed(5);
        selection.move_to(8, true);
        assert_eq!(selection.range(), 5..8);
        selection.move_to(2, true);
        assert_eq!(selection.range(), 2..5);
        selection.move_to(3, false);
        assert_eq!(selection.range(), 3..3);
    }
}
