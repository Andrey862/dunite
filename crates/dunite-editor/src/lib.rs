//! Текстовый движок редактора (SPEC §8).
//!
//! - [`Buffer`] — ropey-буфер с правками и версионированием;
//! - [`Editor`] — буфер + выделение + undo/redo;
//! - [`Movement`] — движения курсора;
//! - авто-продолжение списков и цитат.
//!
//! Модель не зависит от gpui: UI вызывает [`Editor`] и рендерит состояние.
//! M2: декорации Live Preview поверх буфера.

mod auto_continue;
mod buffer;
mod editor;
mod history;
mod movement;
mod selection;

pub use buffer::Buffer;
pub use editor::Editor;
pub use history::{Edit, EditKind, Transaction};
pub use movement::Movement;
pub use selection::Selection;
