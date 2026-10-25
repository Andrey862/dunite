//! Бенчи разбора markdown (бюджет §13 — кадр набора < 8 мс).

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use std::time::Duration;

fn sample_note(paragraphs: usize) -> String {
    let mut text = "---\ntags: [тег]\n---\n\n".to_string();
    for ix in 0..paragraphs {
        text.push_str(&format!(
            "## Раздел {ix}\n\n\
             Абзац с **жирным**, *курсивом*, `кодом` и [[ссылка-{ix}|надписью]]. \
             Тег #тег-{ix} и [сайт](https://example.com/{ix}).\n\n\
             - пункт один\n- [ ] задача {ix}\n\n> цитата {ix}\n\n\
             ```rust\nfn f{ix}() {{}}\n```\n\n"
        ));
    }
    text
}

fn bench_parse(c: &mut Criterion) {
    let small = sample_note(2); // ~1 КБ
    let large = sample_note(100); // ~30 КБ

    c.bench_function("parse_note_small", |b| {
        b.iter(|| dunite_markdown::parse(black_box(&small)))
    });
    c.bench_function("parse_note_large", |b| {
        b.iter(|| dunite_markdown::parse(black_box(&large)))
    });
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(30).measurement_time(Duration::from_secs(2));
    targets = bench_parse
}
criterion_main!(benches);
