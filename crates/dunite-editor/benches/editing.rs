//! Бенчи модели редактора: правки и undo (бюджет §13 — набор < 8 мс).

use criterion::{Criterion, criterion_group, criterion_main};
use dunite_editor::Editor;
use std::hint::black_box;

fn make_note() -> String {
    let mut text = String::new();
    for ix in 0..2000 {
        text.push_str(&format!("Строка {ix} с некоторым текстом для объёма.\n"));
    }
    text
}

fn bench_editing(c: &mut Criterion) {
    let text = make_note();
    c.bench_function("insert_text_typing", |b| {
        b.iter_batched(
            || Editor::from_text(black_box(text.clone())),
            |mut editor| {
                editor.set_cursor(editor.buffer().len_chars() / 2, false);
                for ch in "абвгд".chars() {
                    editor.insert_text(&ch.to_string());
                }
                black_box(editor.text());
            },
            criterion::BatchSize::SmallInput,
        )
    });

    c.bench_function("delete_backward_run", |b| {
        b.iter_batched(
            || {
                let mut editor = Editor::from_text(black_box(text.clone()));
                editor.set_cursor(editor.buffer().len_chars() / 2, false);
                editor
            },
            |mut editor| {
                for _ in 0..20 {
                    editor.delete_backward();
                }
                black_box(editor.text());
            },
            criterion::BatchSize::SmallInput,
        )
    });

    c.bench_function("undo_redo", |b| {
        b.iter_batched(
            || {
                let mut editor = Editor::from_text(black_box(text.clone()));
                editor.set_cursor(0, false);
                editor.insert_text("правка ");
                editor
            },
            |mut editor| {
                editor.undo();
                editor.redo();
                black_box(editor.text());
            },
            criterion::BatchSize::SmallInput,
        )
    });

    c.bench_function("move_cursor_word", |b| {
        b.iter_batched(
            || Editor::from_text(black_box(text.clone())),
            |mut editor| {
                for _ in 0..100 {
                    editor.move_cursor(dunite_editor::Movement::WordRight, false);
                }
                black_box(editor.selection().head);
            },
            criterion::BatchSize::SmallInput,
        )
    });
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(20).measurement_time(std::time::Duration::from_secs(2));
    targets = bench_editing
}
criterion_main!(benches);
