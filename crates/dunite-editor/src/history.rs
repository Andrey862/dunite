//! История правок: undo/redo с группировкой (SPEC §8.1).
//!
//! Группировка: операции одного действия (< 500 мс, непрерывный курсор)
//! сливаются в одну транзакцию — набор текста откатывается целиком.

use std::ops::Range;
use std::time::{Duration, Instant};

use crate::selection::Selection;

/// Одна правка: заменить `range` (координаты ДО правки) на `inserted`,
/// при этом был удалён `deleted`.
#[derive(Debug, Clone)]
pub struct Edit {
    pub range: Range<usize>,
    pub inserted: String,
    pub deleted: String,
}

impl Edit {
    pub fn inserted_chars(&self) -> usize {
        self.inserted.chars().count()
    }

    pub fn deleted_chars(&self) -> usize {
        self.deleted.chars().count()
    }

    /// Правка — чистая вставка (без удаления).
    pub fn is_pure_insert(&self) -> bool {
        self.deleted.is_empty() && !self.inserted.is_empty()
    }

    /// Правка — чистое удаление (без вставки).
    pub fn is_pure_delete(&self) -> bool {
        self.inserted.is_empty() && !self.deleted.is_empty()
    }
}

/// Вид правки — для группировки в undo-истории.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditKind {
    /// Ввод текста (включая Enter с авто-продолжением).
    Typing,
    /// Удаление (Backspace/Delete).
    Delete,
    /// Прочее (вставка из буфера, замена фрагментов) — не группируется.
    Other,
}

/// Транзакция undo: набор правок + выделения до/после.
#[derive(Debug, Clone)]
pub struct Transaction {
    pub edits: Vec<Edit>,
    pub selection_before: Selection,
    pub selection_after: Selection,
    pub kind: EditKind,
    pub timestamp: Instant,
}

/// Группировка транзакций.
const GROUP_WINDOW: Duration = Duration::from_millis(500);

/// История правок.
#[derive(Debug, Default)]
pub struct History {
    undo: Vec<Transaction>,
    redo: Vec<Transaction>,
}

impl History {
    pub fn new() -> Self {
        Self::default()
    }

    /// Записать транзакцию; сливает с предыдущей, если это продолжение
    /// того же действия (см. [`Self::can_merge`]). Очищает redo-стек.
    pub fn push(&mut self, txn: Transaction) {
        self.redo.clear();
        if let Some(last) = self.undo.last_mut() {
            if Self::can_merge(last, &txn) {
                Self::merge(last, txn);
                return;
            }
        }
        self.undo.push(txn);
    }

    /// Начать новую группу: следующая правка не сольётся с предыдущей.
    pub fn break_group(&mut self) {
        if let Some(last) = self.undo.last_mut() {
            last.timestamp = Instant::now()
                .checked_sub(GROUP_WINDOW + Duration::from_secs(1))
                .unwrap_or_else(Instant::now);
        }
    }

    pub fn pop_undo(&mut self) -> Option<Transaction> {
        self.undo.pop()
    }

    pub fn pop_redo(&mut self) -> Option<Transaction> {
        self.redo.pop()
    }

    /// Вернуть транзакцию в redo-стек (вызывается при undo).
    pub fn push_redo(&mut self, txn: Transaction) {
        self.redo.push(txn);
    }

    /// Вернуть транзакцию в undo-стек (вызывается при неудачном redo).
    pub fn push_undo(&mut self, txn: Transaction) {
        self.undo.push(txn);
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Мерж возможен, если транзакции — продолжение одного действия:
    /// - обе типа Typing: чистые вставки, новые символы пишутся встык;
    /// - обе типа Delete: непрерывные цепочки удалений (назад/вперёд).
    fn can_merge(last: &Transaction, new: &Transaction) -> bool {
        if last.kind != new.kind || new.timestamp.duration_since(last.timestamp) > GROUP_WINDOW {
            return false;
        }
        let (Some(prev), Some(next)) = (last.edits.first(), new.edits.first()) else {
            return false;
        };
        if last.edits.len() != 1 || new.edits.len() != 1 {
            return false;
        }
        match new.kind {
            EditKind::Typing => {
                prev.is_pure_insert()
                    && next.is_pure_insert()
                    && next.range.start == prev.range.start + prev.inserted_chars()
            }
            EditKind::Delete => {
                if !prev.is_pure_delete() || !next.is_pure_delete() {
                    return false;
                }
                // удаление назад: следующая правка левее предыдущей, встык
                next.range.end == prev.range.start
                    // удаление вперёд: следующая правка правее, встык
                    || next.range.start == prev.range.end
            }
            EditKind::Other => false,
        }
    }

    fn merge(last: &mut Transaction, new: Transaction) {
        let next = new.edits.into_iter().next().expect("checked in can_merge");
        let prev = &mut last.edits[0];
        if next.range.end == prev.range.start {
            // встык слева (backspace-цепочка)
            prev.range.start = next.range.start;
            prev.deleted = next.deleted + &prev.deleted;
        } else {
            // встык справа (typing или delete-forward)
            prev.range.end = prev.range.start + prev.inserted_chars() + next.deleted_chars();
            prev.inserted.push_str(&next.inserted);
            prev.deleted.push_str(&next.deleted);
        }
        last.selection_after = new.selection_after;
        last.timestamp = new.timestamp;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn insert_txn(range: Range<usize>, text: &str, cursor: usize) -> Transaction {
        Transaction {
            edits: vec![Edit {
                range,
                inserted: text.to_string(),
                deleted: String::new(),
            }],
            selection_before: Selection::collapsed(cursor),
            selection_after: Selection::collapsed(cursor + text.chars().count()),
            kind: EditKind::Typing,
            timestamp: Instant::now(),
        }
    }

    #[test]
    fn typing_merges_when_contiguous() {
        let mut history = History::new();
        history.push(insert_txn(0..0, "a", 0));
        history.push(insert_txn(1..1, "b", 1));
        history.push(insert_txn(2..2, "c", 2));
        let txn = history.pop_undo().unwrap();
        assert_eq!(txn.edits[0].inserted, "abc");
        assert!(history.pop_undo().is_none());
    }

    #[test]
    fn typing_breaks_on_gap() {
        let mut history = History::new();
        history.push(insert_txn(0..0, "a", 0));
        // курсор «убежал»: вставка не встык
        history.push(insert_txn(5..5, "b", 5));
        assert_eq!(history.pop_undo().unwrap().edits[0].inserted, "b");
        assert_eq!(history.pop_undo().unwrap().edits[0].inserted, "a");
    }

    #[test]
    fn delete_runs_merge() {
        let mut history = History::new();
        // backspace-цепочка: "abc|" → удаляем c, b, a
        history.push(Transaction {
            edits: vec![Edit {
                range: 2..3,
                inserted: String::new(),
                deleted: "c".into(),
            }],
            selection_before: Selection::collapsed(3),
            selection_after: Selection::collapsed(2),
            kind: EditKind::Delete,
            timestamp: Instant::now(),
        });
        history.push(Transaction {
            edits: vec![Edit {
                range: 1..2,
                inserted: String::new(),
                deleted: "b".into(),
            }],
            selection_before: Selection::collapsed(2),
            selection_after: Selection::collapsed(1),
            kind: EditKind::Delete,
            timestamp: Instant::now(),
        });
        let txn = history.pop_undo().unwrap();
        assert_eq!(txn.edits[0].range, 1..3);
        assert_eq!(txn.edits[0].deleted, "bc");
    }

    #[test]
    fn other_never_merges() {
        let mut history = History::new();
        history.push(insert_txn(0..0, "a", 0));
        history.push(Transaction {
            kind: EditKind::Other,
            ..insert_txn(1..1, "b", 1)
        });
        assert!(history.pop_undo().is_some());
        assert!(history.pop_undo().is_some());
    }

    #[test]
    fn break_group_prevents_merge() {
        let mut history = History::new();
        history.push(insert_txn(0..0, "a", 0));
        history.break_group();
        history.push(insert_txn(1..1, "b", 1));
        assert!(history.pop_undo().is_some());
        assert!(history.pop_undo().is_some());
    }

    #[test]
    fn redo_cleared_on_new_edit() {
        let mut history = History::new();
        history.push(insert_txn(0..0, "a", 0));
        let txn = history.pop_undo().unwrap();
        history.push_redo(txn);
        assert!(history.can_redo());
        history.push(insert_txn(0..0, "x", 0));
        assert!(!history.can_redo());
    }
}
