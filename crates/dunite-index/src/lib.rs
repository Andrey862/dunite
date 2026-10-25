//! Граф ссылок и тегов (SPEC §7.2, §11).
//!
//! In-memory модель знаний о хранилище: резолвинг ссылок (пути, короткие
//! имена, алиасы), backlinks, теги, заголовки. Полнотекстовый индекс —
//! in-memory постинги (§18: отказ от tantivy), BM25 и сниппеты в search.rs.

mod graph;
mod meta;
mod parse;
mod search;

pub use graph::{BacklinkRef, LinkGraph};
pub use meta::{LinkKind, NoteHeading, NoteMeta, ResolvedLink};
pub use parse::{LARGE_NOTE_BYTES, parse_note};
pub use search::{SearchHit, SearchQuery, Snippet};
