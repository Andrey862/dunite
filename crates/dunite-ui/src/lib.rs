//! UI dunite на gpui (SPEC §9): дерево файлов, табы, редактор, статус-бар.

pub mod actions;
pub mod decor;
pub mod editor_element;
pub mod editor_pane;
pub mod file_tree;
pub mod keys;
pub mod status_bar;
pub mod tab_bar;
pub mod theme;
pub mod workspace;

pub use keys::{global_key_bindings, key_bindings};
pub use workspace::Workspace;
