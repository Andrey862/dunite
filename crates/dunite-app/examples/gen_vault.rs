//! Генератор синтетического хранилища для бенчей и нагрузочных тестов.
//!
//! Использование: `cargo run --release --example gen_vault -- <каталог> <заметок>`

use std::fs;
use std::path::Path;

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = args.next().unwrap_or_else(|| "vault-bench".to_string());
    let count: usize = args
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1000);

    let root = Path::new(&dir);
    fs::create_dir_all(root.join("проекты")).unwrap();
    fs::create_dir_all(root.join("ежедневник")).unwrap();

    for ix in 0..count {
        let folder = match ix % 3 {
            0 => "проекты",
            1 => "ежедневник",
            _ => "",
        };
        let name = format!("заметка-{ix:05}");
        let target1 = format!("заметка-{:05}", (ix * 7 + 3) % count.max(1));
        let target2 = format!("заметка-{:05}", (ix * 13 + 5) % count.max(1));
        let body = format!(
            "---\ntitle: Заметка {ix}\ntags: [тег-{}, проект/{}]\naliases: [псевдоним-{ix}]\n---\n\n\
             # Заголовок заметки {ix}\n\n\
             Это синтетическая заметка номер {ix} для нагрузочного тестирования. \
             Слово для поиска: запрос-{}\n\n\
             Ссылки: [[{target1}]] и [[{target2}|с надписью]]. Тег #тег-{}\n\n\
             ## Раздел\n\n- пункт один\n- пункт два\n- [ ] задача\n\n\
             Смежный текст с повторением слова поиск для проверки частотности.\n",
            ix % 50,
            ix % 10,
            ix % 100,
            ix % 50,
        );
        let path = root.join(folder).join(format!("{name}.md"));
        fs::write(path, body).unwrap();
    }

    println!("создано {count} заметок в {}", root.display());
}
