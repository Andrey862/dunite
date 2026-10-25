//! Бенчи графа знаний: сборка, резолвинг, поиск (бюджеты §13:
//! запрос < 50 мс, сборка 10k < 30 с в фоне).

use criterion::{Criterion, criterion_group, criterion_main};
use dunite_index::LinkGraph;
use dunite_vault::Vault;
use std::fs;
use std::hint::black_box;
use std::time::Duration;

/// Синтетическое хранилище: заметки с frontmatter, ссылками и тегами.
fn generate_vault(root: &std::path::Path, count: usize) {
    fs::create_dir_all(root.join("проекты")).unwrap();
    for ix in 0..count {
        let target1 = format!("заметка-{:05}", (ix * 7 + 3) % count.max(1));
        let target2 = format!("проекты/заметка-{:05}", (ix * 13 + 5) % count.max(1));
        let body = format!(
            "---\ntitle: Заметка {ix}\ntags: [тег-{}, проект/{}]\naliases: [псевдоним-{ix}]\n---\n\n\
             # Заголовок {ix}\n\nТекст с словом запрос-{} и [[{target1}]] и [[{target2}|с надписью]].\n\n\
             - пункт\n- [ ] задача\n",
            ix % 50,
            ix % 10,
            ix % 100,
        );
        let folder = if ix % 2 == 0 { "проекты" } else { "" };
        let name = format!("заметка-{ix:05}.md");
        fs::write(root.join(folder).join(name), body).unwrap();
    }
}

fn bench_knowledge(c: &mut Criterion) {
    let dir = tempfile::tempdir().unwrap();
    generate_vault(dir.path(), 1000);
    let vault = Vault::open(dir.path()).unwrap();

    c.bench_function("graph_build_1k_notes", |b| {
        b.iter(|| black_box(LinkGraph::build(&vault)))
    });

    let graph = LinkGraph::build(&vault);
    c.bench_function("search_term", |b| {
        b.iter(|| black_box(graph.search("запрос-42")))
    });
    c.bench_function("search_and_filter", |b| {
        b.iter(|| black_box(graph.search("текст tag:тег-3")))
    });
    c.bench_function("search_phrase", |b| {
        b.iter(|| black_box(graph.search("\"словом запрос\"")))
    });
    c.bench_function("backlinks_lookup", |b| {
        b.iter(|| {
            black_box(
                graph
                    .backlinks(&dunite_vault::NoteId::new("заметка-00001.md"))
                    .len(),
            )
        })
    });
    c.bench_function("resolve_link", |b| {
        b.iter(|| {
            black_box(graph.resolve(
                &dunite_vault::NoteId::new("заметка-00002.md"),
                "заметка-00009",
            ))
        })
    });
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(10).measurement_time(Duration::from_secs(2));
    targets = bench_knowledge
}
criterion_main!(benches);
