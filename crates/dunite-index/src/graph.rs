//! Граф ссылок и тегов (SPEC §7.2, §11).
//!
//! In-memory модель знаний о хранилище: метаданные заметок (title, алиасы,
//! теги, заголовки), исходящие ссылки с резолвингом и обратные ссылки.
//! Строится по хранилищу целиком, обновляется по одной заметке.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::meta::{LinkKind, NoteMeta};
use crate::parse::parse_note;
use crate::search::{self, SearchHit, Snippet};
use dunite_vault::{NoteId, Vault};

/// Обратная ссылка: `source` ссылается на цель.
#[derive(Debug, Clone)]
pub struct BacklinkRef {
    pub source: NoteId,
    /// Char-диапазон ссылки в исходнике.
    pub range: std::ops::Range<usize>,
    /// Char-диапазон текста цели — для перезаписи при переименовании.
    pub target_range: std::ops::Range<usize>,
    /// Текст цели, как написан в источнике.
    pub target_text: String,
    /// Надпись `[[target|display]]`, если есть.
    pub display: Option<String>,
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
    /// Полнотекстовые постинги (SPEC §11): токен → (заметка, частота).
    postings: HashMap<String, Vec<(NoteId, u32)>>,
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
            graph.index_text(&meta.id, &meta.body);
            graph.metas.insert(meta.id.clone(), meta);
        }
        graph.resolve_all();
        graph
    }

    /// Перечитывает одну заметку (после правки/переименования).
    pub fn update_note(&mut self, vault: &Vault, id: &NoteId) {
        // убрать старые записи: имена, постинги и обратные ссылки
        self.unindex_names(id);
        self.unindex_text(id);
        self.backlinks
            .values_mut()
            .for_each(|refs| refs.retain(|r| &r.source != id));

        let meta = parse_note(vault, id);
        self.index_name(&meta.id, meta.id.as_str());
        self.index_name(&meta.id, meta.id.file_stem());
        for alias in &meta.aliases {
            self.index_name(&meta.id, alias);
        }
        self.index_text(&meta.id, &meta.body);
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
        self.unindex_text(id);
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

    /// Полнотекстовый поиск (SPEC §11).
    ///
    /// Язык запросов: слова (AND), `"фразы"`, фильтры `tag:`, `path:`,
    /// `link:`. Сортировка — по убыванию релевантности (BM25).
    pub fn search(&self, raw: &str) -> Vec<SearchHit> {
        let query = search::parse_query(raw);
        let nothing_to_find = query.is_empty()
            && query.tags.is_empty()
            && query.paths.is_empty()
            && query.links.is_empty();
        if nothing_to_find {
            return Vec::new();
        }

        // статистика запроса — по одному проходу, без клонирования строк:
        // иначе широкие запросы на больших хранилищах съедают память
        let term_docs: Vec<HashMap<&NoteId, u32>> = query
            .terms
            .iter()
            .map(|term| self.docs_for_term(term))
            .collect();
        let phrase_hits: Vec<HashSet<&NoteId>> = query
            .phrases
            .iter()
            .map(|phrase| {
                self.metas
                    .iter()
                    .filter(|(_, meta)| meta.body_lower.contains(phrase.as_str()))
                    .map(|(id, _)| id)
                    .collect()
            })
            .collect();

        // кандидаты: пересечение постингов по словам (AND)
        let mut candidates: Option<HashSet<&NoteId>> = None;
        for docs in &term_docs {
            let set: HashSet<&NoteId> = docs.keys().copied().collect();
            candidates = Some(match candidates {
                Some(previous) => previous.intersection(&set).copied().collect(),
                None => set,
            });
            if candidates.as_ref().is_some_and(|set| set.is_empty()) {
                return Vec::new();
            }
        }
        let candidates: HashSet<&NoteId> = match candidates {
            Some(set) => set,
            None => self.metas.keys().collect(),
        };

        let total = self.metas.len();
        let needles = query.needles();
        let mut hits: Vec<SearchHit> = Vec::new();
        for id in candidates {
            let meta = &self.metas[id];
            let body_lower = &meta.body_lower;

            if !phrase_hits.iter().all(|hits| hits.contains(id)) {
                continue;
            }
            if !query.tags.iter().all(|tag| {
                meta.tags
                    .iter()
                    .any(|t| t == tag || t.starts_with(&format!("{tag}/")))
            }) {
                continue;
            }
            if !query
                .paths
                .iter()
                .all(|path| id.as_str().to_lowercase().contains(path.as_str()))
            {
                continue;
            }
            if !query.links.iter().all(|target| {
                meta.links.iter().any(|link| {
                    link.target_text.to_lowercase().contains(target.as_str())
                        || self.resolve(id, &link.target_text).is_some_and(|resolved| {
                            resolved.file_stem().to_lowercase() == *target
                                || resolved.as_str().to_lowercase() == *target
                        })
                })
            }) {
                continue;
            }

            // релевантность: BM25-подобная сумма по словам и фразам
            let mut score = 0f32;
            for docs in &term_docs {
                let tf = docs.get(id).copied().unwrap_or(0);
                score += search::term_score(tf, docs.len(), total);
            }
            for (phrase, phrase_docs) in query.phrases.iter().zip(&phrase_hits) {
                let tf = body_lower.matches(phrase.as_str()).count() as u32;
                score += search::term_score(tf.max(1), phrase_docs.len(), total);
            }

            // сниппеты строятся ПОСЛЕ сортировки — только для топа
            hits.push(SearchHit {
                id: (*id).clone(),
                score,
                snippets: Vec::new(),
            });
        }

        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.id.cmp(&b.id))
        });
        hits.truncate(search::MAX_RESULTS);
        for hit in &mut hits {
            hit.snippets = match self.metas.get(&hit.id) {
                Some(meta) if needles.is_empty() => meta
                    .body
                    .lines()
                    .next()
                    .map(|line| {
                        vec![Snippet {
                            line: 0,
                            text: line.to_string(),
                            ranges: Vec::new(),
                        }]
                    })
                    .unwrap_or_default(),
                Some(meta) => search::build_snippets(&meta.body, &needles),
                None => Vec::new(),
            };
        }
        hits
    }

    /// Заметки с термином: подстрочное совпадение токенов (как в Obsidian —
    /// «ежик» находит «ежиком»). Возвращает суммарную частоту по заметке.
    fn docs_for_term(&self, term: &str) -> HashMap<&NoteId, u32> {
        if term.is_empty() {
            return HashMap::new();
        }
        let mut totals: HashMap<&NoteId, u32> = HashMap::new();
        for (token, entries) in &self.postings {
            if !token.contains(term) {
                continue;
            }
            for (id, count) in entries {
                *totals.entry(id).or_default() += count;
            }
        }
        totals
    }

    /// Переименовывает или перемещает ПАПКУ (SPEC §7.2).
    ///
    /// Переписываются только path-ссылки (`[[Папка/Заметка]]`) на заметки
    /// внутри папки; короткие `[[Заметка]]` и алиасы не трогаем — они
    /// резолвятся по-новой сами. Возвращает число обновлённых ссылок.
    pub fn rename_folder(&mut self, vault: &Vault, from: &str, to: &str) -> std::io::Result<usize> {
        use std::io;

        let from = from.trim_end_matches('/');
        let to = to.trim_end_matches('/');
        let from_path = vault.absolute(from);
        let to_path = vault.absolute(to);
        if !from_path.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("нет папки {from}"),
            ));
        }
        if to_path.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{to} уже существует"),
            ));
        }

        // 1. переписать path-ссылки на заметки внутри папки
        let prefix = format!("{from}/");
        let new_prefix = format!("{to}/");
        let mut by_source: HashMap<NoteId, Vec<BacklinkRef>> = HashMap::new();
        for (source, meta) in &self.metas {
            for link in &meta.links {
                if link.target_text.starts_with(&prefix) {
                    by_source
                        .entry(source.clone())
                        .or_default()
                        .push(BacklinkRef {
                            source: source.clone(),
                            range: link.range.clone(),
                            target_range: link.target_range.clone(),
                            target_text: link.target_text.clone(),
                            display: link.display.clone(),
                            context: link.context.clone(),
                            line: link.line,
                        });
                }
            }
        }

        let mut updated = 0usize;
        let mut rewritten: Vec<NoteId> = Vec::new();
        for (source, mut refs) in by_source {
            let Ok(text) = vault.read_note(source.as_str()) else {
                continue;
            };
            let mut chars: Vec<char> = text.chars().collect();
            refs.sort_by_key(|ref_| ref_.target_range.start);
            for ref_ in refs.iter().rev() {
                if ref_.target_range.end > chars.len() {
                    continue;
                }
                let replacement = format!("{new_prefix}{}", &ref_.target_text[prefix.len()..]);
                chars.splice(ref_.target_range.clone(), replacement.chars());
                updated += 1;
            }
            let new_text: String = chars.into_iter().collect();
            std::fs::write(vault.absolute(source.as_str()), new_text)?;
            rewritten.push(source);
        }

        // 2. переместить папку
        if let Some(parent) = to_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::rename(&from_path, &to_path)?;

        // 3. обновить граф: перечитать переписанные источники и перемещённые
        for source in &rewritten {
            self.update_note(vault, source);
        }
        let moved: Vec<String> = vault
            .notes_under(from)
            .iter()
            .map(|id| {
                let suffix = &id.as_str()[from.len()..];
                format!("{to}{suffix}")
            })
            .collect();
        for old in vault.notes_under(from) {
            self.remove_note(&old);
        }
        for new_id in moved {
            self.update_note(vault, &NoteId::new(new_id));
        }
        self.backlinks.clear();
        self.resolve_all();
        Ok(updated)
    }

    /// Переименовывает заметку и переписывает все ссылающиеся ссылки
    /// (SPEC §7.2: «пересохранение ссылок при переименовании обязательно»).
    ///
    /// Заменяется только текст цели ссылки (`[[старое|надпись]]` →
    /// `[[новое|надпись]]`). Возвращает число обновлённых ссылок.
    pub fn rename_note(
        &mut self,
        vault: &Vault,
        from: &NoteId,
        to: &NoteId,
    ) -> std::io::Result<usize> {
        use std::io;

        let from_path = vault.absolute(from.as_str());
        let to_path = vault.absolute(to.as_str());
        if !from_path.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("нет файла {}", from.as_str()),
            ));
        }
        if to_path.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("файл {} уже существует", to.as_str()),
            ));
        }

        // 1. переписать ссылающиеся заметки
        let mut by_source: HashMap<NoteId, Vec<BacklinkRef>> = HashMap::new();
        for ref_ in self.backlinks(from) {
            by_source
                .entry(ref_.source.clone())
                .or_default()
                .push(ref_.clone());
        }
        let new_target: String = to.as_str().trim_end_matches(".md").to_string();
        let mut updated = 0usize;
        let mut rewritten: Vec<NoteId> = Vec::new();
        for (source, mut refs) in by_source {
            let Ok(text) = vault.read_note(source.as_str()) else {
                continue;
            };
            let mut chars: Vec<char> = text.chars().collect();
            // с конца — чтобы смещения остались валидными
            refs.sort_by_key(|ref_| ref_.target_range.start);
            for ref_ in refs.iter().rev() {
                if ref_.target_range.end > chars.len() {
                    continue;
                }
                // ссылки по алиасу без надписи сохраняют видимый текст:
                // [[Home]] → [[Новое|Home]] (как в Obsidian); канонические
                // (имя файла/путь) и уже имеющие надпись переписываются целиком
                let replacement = match &ref_.display {
                    Some(_) => new_target.clone(),
                    None if is_canonical_target(&ref_.target_text, from) => new_target.clone(),
                    None => format!("{new_target}|{}", ref_.target_text),
                };
                chars.splice(ref_.target_range.clone(), replacement.chars());
                updated += 1;
            }
            let new_text: String = chars.into_iter().collect();
            std::fs::write(vault.absolute(source.as_str()), new_text)?;
            rewritten.push(source);
        }

        // 2. переименовать сам файл
        if let Some(parent) = to_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::rename(&from_path, &to_path)?;

        // 3. обновить граф: перечитать переписанные заметки и цель
        self.remove_note(from);
        for source in &rewritten {
            self.update_note(vault, source);
        }
        self.update_note(vault, to);
        Ok(updated)
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

    /// Полнотекстовые постинги заметки (SPEC §11).
    fn index_text(&mut self, id: &NoteId, body: &str) {
        let mut frequencies: HashMap<String, u32> = HashMap::new();
        for token in search::tokenize(body) {
            *frequencies.entry(token).or_default() += 1;
        }
        for (token, count) in frequencies {
            self.postings
                .entry(token)
                .or_default()
                .push((id.clone(), count));
        }
    }

    fn unindex_text(&mut self, id: &NoteId) {
        self.postings.retain(|_, entries| {
            entries.retain(|(entry_id, _)| entry_id != id);
            !entries.is_empty()
        });
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
                        target_range: link.target_range,
                        target_text: link.target_text,
                        display: link.display,
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

/// Цель ссылки — каноническое имя заметки (имя файла или путь), а не алиас.
fn is_canonical_target(target: &str, id: &NoteId) -> bool {
    let target = target.trim().to_lowercase();
    let stem = id.file_stem().to_lowercase();
    let path = id.as_str().to_lowercase();
    let path_no_ext = path.strip_suffix(".md").unwrap_or(&path);
    target == stem || target == path || target == path_no_ext
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
            "[[Несуществующая]] и [[Главная|домой]] и [[Home]]\n",
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
    fn rename_folder_rewrites_path_links_only() {
        let (_dir, mut graph, mut vault) = graph();
        fs::write(
            vault.absolute("Главная.md"),
            "см. [[projects/Бета]] и [[Alpha]] и [[Home]]\n",
        )
        .unwrap();
        vault.rescan().unwrap();
        graph.update_note(&vault, &NoteId::new("Главная.md"));

        let updated = graph.rename_folder(&vault, "projects", "архив").unwrap();
        // path-ссылки: [[projects/Бета]] из Главной и из projects/Alpha.md
        assert_eq!(updated, 2);

        // path-ссылка переписана, короткая и алиасная — нет (резолвятся сами)
        let text = vault.read_note("Главная.md").unwrap();
        assert!(text.contains("[[архив/Бета]]"), "{text}");
        assert!(text.contains("[[Alpha]]"), "{text}");
        assert!(text.contains("[[Home]]"), "{text}");
        assert!(vault.absolute("архив/Бета.md").is_file());

        // резолвинг после переименования работает
        assert_eq!(
            graph.resolve(&NoteId::new("Главная.md"), "архив/Бета"),
            Some(NoteId::new("архив/Бета.md"))
        );
    }

    #[test]
    fn rename_folder_refuses_existing_target() {
        let (_dir, mut graph, vault) = graph();
        fs::create_dir_all(vault.absolute("архив")).unwrap();
        assert!(graph.rename_folder(&vault, "projects", "архив").is_err());
        assert!(graph.rename_folder(&vault, "нет такой", "новая").is_err());
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

    #[test]
    fn rename_note_rewrites_backlinks_preserving_display() {
        let (_dir, mut graph, mut vault) = graph();
        fs::write(
            vault.absolute("projects/Бета.md"),
            "См. [[Alpha]] и [[Alpha|с надписью]]\n",
        )
        .unwrap();
        vault.rescan().unwrap();
        graph.update_note(&vault, &NoteId::new("projects/Бета.md"));

        let from = NoteId::new("projects/Alpha.md");
        let to = NoteId::new("projects/Гамма.md");
        let updated = graph.rename_note(&vault, &from, &to).unwrap();
        assert!(updated >= 2, "переписано ссылок: {updated}");

        let text = vault.read_note("projects/Бета.md").unwrap();
        assert!(text.contains("[[projects/Гамма]]"), "{text}");
        assert!(text.contains("[[projects/Гамма|с надписью]]"), "{text}");
        assert!(!text.contains("Alpha"), "{text}");

        assert_eq!(
            graph.resolve(&NoteId::new("projects/Бета.md"), "projects/Гамма"),
            Some(to.clone())
        );
        assert!(
            graph
                .backlinks(&to)
                .iter()
                .any(|r| r.source.as_str() == "projects/Бета.md")
        );
    }

    #[test]
    fn rename_refuses_existing_target() {
        let (_dir, mut graph, vault) = graph();
        let err = graph.rename_note(
            &vault,
            &NoteId::new("projects/Бета.md"),
            &NoteId::new("Главная.md"),
        );
        assert!(err.is_err());
    }

    #[test]
    fn rename_preserves_alias_display_text() {
        let (_dir, mut graph, vault) = graph();
        let from = NoteId::new("Главная.md");
        let to = NoteId::new("Дом.md");
        graph.rename_note(&vault, &from, &to).unwrap();

        // алиасная ссылка без надписи сохраняет видимый текст: [[Home]] → [[Дом|Home]]
        let text = vault.read_note("Вагон.md").unwrap();
        assert!(text.contains("[[Дом|Home]]"), "{text}");
        // ссылка с надписью переписывается как есть
        assert!(text.contains("[[Дом|домой]]"), "{text}");

        // каноническая ссылка (имя файла) переписывается без надписи
        let alpha = vault.read_note("projects/Alpha.md").unwrap();
        assert!(alpha.contains("Ссылка на [[Дом]]"), "{alpha}");
    }

    // ── поиск (SPEC §11) ────────────────────────────────────────────────

    fn search_fixture() -> (tempfile::TempDir, LinkGraph, Vault) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("projects")).unwrap();
        fs::write(
            root.join("Первая.md"),
            "---\ntags: [работа]\n---\n\nдизайн системы поиска\nвторая строка\n",
        )
        .unwrap();
        fs::write(
            root.join("projects/Вторая.md"),
            "поиска много\nпоиска и дизайн\n",
        )
        .unwrap();
        fs::write(root.join("Третья.md"), "совсем другое\n").unwrap();
        fs::write(root.join("Четвёртая.md"), "ссылка на [[Первая]]\n").unwrap();
        let vault = Vault::open(root).unwrap();
        let graph = LinkGraph::build(&vault);
        (dir, graph, vault)
    }

    #[test]
    fn search_and_semantics_with_filters() {
        let (_dir, graph, _vault) = search_fixture();

        // AND по словам: оба слова есть в двух заметках
        let hits = graph.search("поиска дизайн");
        let mut ids: Vec<&str> = hits.iter().map(|hit| hit.id.as_str()).collect();
        ids.sort();
        // байтовый порядок: «projects/...» < «Первая.md»
        assert_eq!(ids, vec!["projects/Вторая.md", "Первая.md"]);

        // фраза — только точное соседство
        let hits = graph.search("\"поиска много\"");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id.as_str(), "projects/Вторая.md");

        // фильтр тега
        let hits = graph.search("tag:работа");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id.as_str(), "Первая.md");

        // фильтр пути + слово
        let hits = graph.search("поиска path:projects");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id.as_str(), "projects/Вторая.md");

        // фильтр ссылки
        let hits = graph.search("link:Первая");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id.as_str(), "Четвёртая.md");

        // пустой запрос результатов не даёт
        assert!(graph.search("   ").is_empty());
    }

    #[test]
    fn search_ranks_by_frequency() {
        let (_dir, graph, _vault) = search_fixture();
        let hits = graph.search("поиска");
        assert_eq!(hits.len(), 2);
        // «Вторая» содержит слово дважды — выше
        assert_eq!(hits[0].id.as_str(), "projects/Вторая.md");
        assert!(hits[0].score > hits[1].score);
    }

    #[test]
    fn search_snippets_have_match_ranges() {
        let (_dir, graph, _vault) = search_fixture();
        let hits = graph.search("дизайн");
        assert!(!hits[0].snippets.is_empty());
        assert!(
            hits[0]
                .snippets
                .iter()
                .any(|snippet| !snippet.ranges.is_empty())
        );
    }

    #[test]
    fn search_reindexes_on_update() {
        let (_dir, mut graph, vault) = search_fixture();
        assert!(graph.search("ежик").is_empty());
        fs::write(vault.absolute("Третья.md"), "новый текст с ежиком\n").unwrap();
        graph.update_note(&vault, &NoteId::new("Третья.md"));
        let hits = graph.search("ежик");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id.as_str(), "Третья.md");
    }
}
