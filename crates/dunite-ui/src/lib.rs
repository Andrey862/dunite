//! UI dunite на gpui (SPEC §9): дерево файлов, табы, панель текста, статус-бар.

pub mod actions;
pub mod file_tree;
pub mod status_bar;
pub mod tab_bar;
pub mod text_pane;
pub mod theme;
pub mod workspace;

pub use workspace::Workspace;
