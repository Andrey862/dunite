//! Граф ссылок и тегов (SPEC §7.2, §11).
//!
//! In-memory модель знаний о хранилище: резолвинг ссылок (пути, короткие
//! имена, алиасы), backlinks, теги, заголовки. Полнотекстовый индекс
//! (tantivy) — M4.

mod graph;
mod meta;
mod parse;

pub use graph::{BacklinkRef, LinkGraph};
pub use meta::{LinkKind, NoteHeading, NoteMeta, ResolvedLink};
pub use parse::parse_note;
