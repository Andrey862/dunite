//! Граф ссылок и тегов (SPEC §7.2, §11).
//!
//! In-memory модель знаний о хранилище: метаданные заметок (title, алиасы,
//! теги, заголовки), исходящие ссылки с резолвингом и обратные ссылки.
//! Строится по хранилищу целиком, обновляется по одной заметке.

use std::collections::{BTreeMap, HashMap};

use crate::meta::{LinkKind, NoteMeta};
use crate::parse::parse_note;
use dunite_vault::{NoteId, Vault};

/// Обратная ссылка: `source` ссылается на цель.
#[derive(Debug, Clone)]
pub struct BacklinkRef {
    pub source: NoteId,
    /// Char-диапазон ссылки в исходнике.
    pub range: std::ops::Range<usize>,
    /// Строка со ссылкой — фрагмент контекста для панели.
    pub context: String,
    /// Строка ссылки в исходнике (для клика и навигации).
    pub line: usize,
}

/// Граф связей хранилища.
#[derive(Debug, Default)]
pub struct LinkGraph {
    metas: HashMap<NoteId, NoteMeta>,
    /// Обратные ссылки: цель → кто ссылается.
    backlinks: HashMap<NoteId, Vec<BacklinkRef>>,
    /// Индекс имён для резолвинга: нормализованное имя → кандидаты.
    name_index: HashMap<String, Vec<NoteId>>,
}

impl LinkGraph {
    /// Полный разбор хранилища.
    pub fn build(vault: &Vault) -> Self {
        let mut graph = Self::default();
        for entry in vault.iter_notes() {
            let id = entry.note_id();
            let meta = parse_note(vault, &id);
            graph.index_name(&meta.id, meta.id.as_str());
            graph.index_name(&meta.id, meta.id.file_stem());
            for alias in &meta.aliases {
                graph.index_name(&meta.id, alias);
            }
            graph.metas.insert(meta.id.clone(), meta);
        }
        graph.resolve_all();
        graph
    }

    /// Перечитывает одну заметку (после правки/переименования).
    pub fn update_note(&mut self, vault: &Vault, id: &NoteId) {
        // убрать старые записи: имена и обратные ссылки от этой заметки
        self.unindex_names(id);
        self.backlinks
            .values_mut()
            .for_each(|refs| refs.retain(|r| &r.source != id));

        let meta = parse_note(vault, id);
        self.index_name(&meta.id, meta.id.as_str());
        self.index_name(&meta.id, meta.id.file_stem());
        for alias in &meta.aliases {
            self.index_name(&meta.id, alias);
        }
        self.metas.insert(meta.id.clone(), meta);

        // резолвим ссылки заново: изменившиеся имена влияют на чужие ссылки,
        // поэтому проще пере-резолвить всё (это дёшево — без чтения файлов)
        self.backlinks.clear();
        self.resolve_all();
    }

    /// Полностью снимает заметку с учёта (при удалении файла).
    pub fn remove_note(&mut self, id: &NoteId) {
        self.metas.remove(id);
        self.unindex_names(id);
        self.backlinks
            .values_mut()
            .for_each(|refs| refs.retain(|r| &r.source != id));
        self.backlinks.remove(id);
    }

    /// Резолвит цель ссылки из заметки `from` (SPEC §7.2).
    pub fn resolve(&self, from: &NoteId, target_text: &str) -> Option<NoteId> {
        let target = target_text.trim();
        if target.is_empty() {
            return None;
        }

        // 1. точное совпадение относительного пути
        let as_path = if target.ends_with(".md") {
            target.to_string()
        } else {
            format!("{target}.md")
        };
        if let Some(meta) = self.metas.get(&NoteId::new(&as_path)) {
            return Some(meta.id.clone());
        }
        if let Some(meta) = self.metas.get(&NoteId::new(target)) {
            return Some(meta.id.clone());
        }

        // 2/3. короткое имя, суффикс пути, алиас — по индексу имён
        let key = normalize(target);
        let candidates = self.name_index.get(&key)?;
        match candidates.len() {
            1 => Some(candidates[0].clone()),
            _ => {
                // несколько кандидатов: предпочитаем суффикс пути относительно
                // каталога заметки-источника
                let from_dir = from.parent().unwrap_or("");
                candidates
                    .iter()
                    .find(|id| {
                        let path = id.as_str().to_lowercase();
                        path.starts_with(&format!("{from_dir}/")) || from_dir.is_empty()
                    })
                    .cloned()
                    .or_else(|| candidates.first().cloned())
            }
        }
    }

    /// Метаданные заметки.
    pub fn meta(&self, id: &NoteId) -> Option<&NoteMeta> {
        self.metas.get(id)
    }

    /// Обратные ссылки на заметку.
    pub fn backlinks(&self, id: &NoteId) -> &[BacklinkRef] {
        self.backlinks.get(id).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Все теги хранилища с числом заметок.
    pub fn tags(&self) -> BTreeMap<String, usize> {
        let mut out: BTreeMap<String, usize> = BTreeMap::new();
        for meta in self.metas.values() {
            for tag in &meta.tags {
                *out.entry(tag.clone()).or_default() += 1;
            }
        }
        out
    }

    /// Заметки с тегом.
    pub fn notes_with_tag(&self, tag: &str) -> Vec<NoteId> {
        let tag = normalize(tag);
        self.metas
            .values()
            .filter(|meta| meta.tags.iter().any(|t| normalize(t) == tag))
            .map(|meta| meta.id.clone())
            .collect()
    }

    /// Заметки для quick switcher: (id, заголовок).
    pub fn notes(&self) -> impl Iterator<Item = (&NoteId, &NoteMeta)> {
        self.metas.iter()
    }

    // ── внутреннее ────────────────────────────────────────────────────────

    fn index_name(&mut self, id: &NoteId, name: &str) {
        let key = normalize(name);
        if key.is_empty() {
            return;
        }
        let entry = self.name_index.entry(key).or_default();
        if !entry.contains(id) {
            entry.push(id.clone());
        }
    }

    fn unindex_names(&mut self, id: &NoteId) {
        self.name_index.retain(|_, ids| {
            ids.retain(|candidate| candidate != id);
            !ids.is_empty()
        });
    }

    /// Резолвит все исходящие ссылки и строит обратные.
    fn resolve_all(&mut self) {
        let metas: Vec<(NoteId, Vec<_>)> = self
            .metas
            .iter()
            .map(|(id, meta)| (id.clone(), meta.links.clone()))
            .collect();
        for (source, links) in metas {
            for link in links {
                if link.kind == LinkKind::Embed && !link.target_text.ends_with(".md") {
                    // вложения-картинки не создают backlinks на заметки
                    continue;
                }
                if let Some(target) = self.resolve(&source, &link.target_text) {
                    if target == source {
                        continue;
                    }
                    self.backlinks.entry(target).or_default().push(BacklinkRef {
                        source: source.clone(),
                        range: link.range,
                        context: link.context,
                        line: link.line,
                    });
                }
            }
        }
        for refs in self.backlinks.values_mut() {
            refs.sort_by(|a, b| a.source.cmp(&b.source).then(a.line.cmp(&b.line)));
        }
    }
}

fn normalize(name: &str) -> String {
    name.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fixture() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("projects")).unwrap();
        fs::write(
            root.join("Главная.md"),
            "---\ntitle: Главная страница\naliases: [Home, Старт]\ntags: [meta]\n---\n\nСм. [[Alpha]] и [[Бета]]. Тег #главная\n",
        )
        .unwrap();
        fs::write(
            root.join("projects/Alpha.md"),
            "Ссылка на [[Главная]] и на [[projects/Бета]].\n\n#проект/внутренний\n",
        )
        .unwrap();
        fs::write(root.join("projects/Бета.md"), "Обратно: [[Alpha]]\n").unwrap();
        fs::write(root.join("Проект.md"), "[[Alpha]] дубликат имени\n").unwrap();
        fs::write(
            root.join("Вагон.md"),
            "[[Несуществующая]] и [[Главная|домой]]\n",
        )
        .unwrap();
        let vault = Vault::open(root).unwrap();
        (dir, vault)
    }

    fn graph() -> (tempfile::TempDir, LinkGraph, Vault) {
        let (dir, vault) = fixture();
        let graph = LinkGraph::build(&vault);
        (dir, graph, vault)
    }

    #[test]
    fn resolves_path_stem_and_alias() {
        let (_dir, graph, _vault) = graph();
        let from = NoteId::new("Главная.md");
        assert_eq!(
            graph.resolve(&from, "projects/Бета"),
            Some(NoteId::new("projects/Бета.md"))
        );
        assert_eq!(
            graph.resolve(&from, "Главная"),
            Some(NoteId::new("Главная.md"))
        );
        assert_eq!(
            graph.resolve(&from, "Главная.md"),
            Some(NoteId::new("Главная.md"))
        );
        assert_eq!(
            graph.resolve(&from, "Home"),
            Some(NoteId::new("Главная.md"))
        );
        assert_eq!(
            graph.resolve(&from, "Старт"),
            Some(NoteId::new("Главная.md"))
        );
        assert_eq!(graph.resolve(&from, "Несуществующая"), None);
    }

    #[test]
    fn resolves_stem_relative_to_source_dir() {
        let (_dir, graph, _vault) = graph();
        // [[Alpha]] из projects/Бета.md указывает на projects/Alpha.md,
        // а не на Проект.md (тот тоже содержит 'alpha' в тексте, но имя другое)
        let from = NoteId::new("projects/Бета.md");
        assert_eq!(
            graph.resolve(&from, "Alpha"),
            Some(NoteId::new("projects/Alpha.md"))
        );
    }

    #[test]
    fn backlinks_collected_with_context() {
        let (_dir, graph, _vault) = graph();
        let refs = graph.backlinks(&NoteId::new("projects/Alpha.md"));
        let sources: Vec<&str> = refs.iter().map(|r| r.source.as_str()).collect();
        assert!(sources.contains(&"Главная.md"));
        assert!(sources.contains(&"Проект.md"));
        assert!(sources.contains(&"projects/Бета.md"));
        assert!(refs.iter().all(|r| !r.context.is_empty()));
    }

    #[test]
    fn tags_merged_from_frontmatter_and_inline() {
        let (_dir, graph, _vault) = graph();
        let tags = graph.tags();
        assert_eq!(tags.get("meta"), Some(&1));
        assert_eq!(tags.get("главная"), Some(&1));
        assert_eq!(tags.get("проект/внутренний"), Some(&1));
        assert_eq!(
            graph.notes_with_tag("meta"),
            vec![NoteId::new("Главная.md")]
        );
    }

    #[test]
    fn title_and_aliases_from_frontmatter() {
        let (_dir, graph, _vault) = graph();
        let meta = graph.meta(&NoteId::new("Главная.md")).unwrap();
        assert_eq!(meta.title, "Главная страница");
        assert_eq!(meta.aliases, vec!["Home".to_string(), "Старт".to_string()]);
    }

    #[test]
    fn update_note_reindexes() {
        let (_dir, mut graph, vault) = graph();
        let id = NoteId::new("projects/Бета.md");
        fs::write(
            vault.absolute("projects/Бета.md"),
            "Теперь тут [[Главная]]\n",
        )
        .unwrap();
        graph.update_note(&vault, &id);

        // старая обратная ссылка на Alpha исчезла
        let refs = graph.backlinks(&NoteId::new("projects/Alpha.md"));
        assert!(!refs.iter().any(|r| r.source == id));
        // новая на Главная появилась
        let refs = graph.backlinks(&NoteId::new("Главная.md"));
        assert!(refs.iter().any(|r| r.source == id));
    }

    #[test]
    fn rename_via_update_relinks() {
        let (_dir, mut graph, mut vault) = graph();
        // переименование: projects/Бета.md → projects/Вечер.md
        fs::rename(
            vault.absolute("projects/Бета.md"),
            vault.absolute("projects/Вечер.md"),
        )
        .unwrap();
        vault.rescan().unwrap();
        graph.remove_note(&NoteId::new("projects/Бета.md"));
        let new_id = NoteId::new("projects/Вечер.md");
        graph.update_note(&vault, &new_id);

        assert_eq!(
            graph.resolve(&NoteId::new("Главная.md"), "Вечер"),
            Some(new_id.clone())
        );
    }
}
