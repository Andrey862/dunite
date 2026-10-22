//! Разбор метаданных заметки: frontmatter, заголовки, ссылки, теги.

use dunite_markdown::{BlockKind, Inline};
use dunite_vault::{NoteId, Vault};
use serde::Deserialize;

use crate::meta::{LinkKind, NoteHeading, NoteMeta, ResolvedLink};

/// Разбирает заметку хранилища в метаданные.
pub fn parse_note(vault: &Vault, id: &NoteId) -> NoteMeta {
    let text = vault.read_note(id.as_str()).unwrap_or_default();
    let chars: Vec<char> = text.chars().collect();
    let parsed = dunite_markdown::parse(&text);

    let mut title = id.file_stem().to_string();
    let mut aliases: Vec<String> = Vec::new();
    let mut tags: Vec<String> = Vec::new();
    let mut headings = Vec::new();
    let mut links = Vec::new();

    for block in &parsed.blocks {
        if let BlockKind::Frontmatter = block.kind {
            if let Some(frontmatter) = parse_frontmatter(&chars[block.range.clone()]) {
                if let Some(fm_title) = frontmatter.title {
                    title = fm_title.trim().to_string();
                }
                aliases = frontmatter.aliases;
                // теги регистронезависимы и хранятся в lowercase (SPEC §7.4)
                tags = frontmatter
                    .tags
                    .into_iter()
                    .map(|tag| tag.trim().trim_start_matches('#').to_lowercase())
                    .filter(|tag| !tag.is_empty())
                    .collect();
            }
        }
        if let BlockKind::Heading { level, marker } = &block.kind {
            let raw: String = chars[marker.end..block.range.end].iter().collect();
            headings.push(NoteHeading {
                level: *level,
                text: raw.trim().to_string(),
                offset: block.range.start,
            });
        }

        for inline in &block.inlines {
            match inline {
                Inline::WikiLink {
                    range,
                    target,
                    display,
                    embed,
                } => links.push(ResolvedLink {
                    target_text: collect_chars(&chars, target),
                    target_range: target.clone(),
                    display: display.as_ref().map(|range| collect_chars(&chars, range)),
                    range: range.clone(),
                    line: line_of(&chars, range.start),
                    context: line_text(&chars, range.start),
                    kind: if *embed {
                        LinkKind::Embed
                    } else {
                        LinkKind::Wiki
                    },
                }),
                Inline::MarkdownLink {
                    range,
                    target,
                    display,
                    image,
                } => links.push(ResolvedLink {
                    target_text: collect_chars(&chars, target),
                    target_range: target.clone(),
                    display: display.as_ref().map(|range| collect_chars(&chars, range)),
                    range: range.clone(),
                    line: line_of(&chars, range.start),
                    context: line_text(&chars, range.start),
                    kind: if *image {
                        LinkKind::Embed
                    } else {
                        LinkKind::Markdown
                    },
                }),
                Inline::Tag { range } => {
                    let tag: String = collect_chars(&chars, &(range.start + 1..range.end));
                    let tag = tag.trim().trim_start_matches('#').to_lowercase();
                    if !tag.is_empty() {
                        tags.push(tag);
                    }
                }
                _ => {}
            }
        }
    }

    tags.sort();
    tags.dedup();

    NoteMeta {
        id: id.clone(),
        title,
        aliases,
        tags,
        headings,
        links,
    }
}

// ── frontmatter (SPEC §7.3) ──────────────────────────────────────────────

#[derive(Debug, Default, Deserialize)]
struct Frontmatter {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    aliases: StringOrList,
    #[serde(default)]
    tags: StringOrList,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum StringOrList {
    One(String),
    Many(Vec<String>),
    Missing,
}

impl Default for StringOrList {
    fn default() -> Self {
        Self::Missing
    }
}

impl StringOrList {
    /// Obsidian допускает `tags: a, b` одной строкой и списком.
    /// Регистр сохраняется: теги нормализуются при разборе заметки,
    /// алиасы — имена и не приводятся к нижнему регистру.
    fn into_vec(self) -> Vec<String> {
        match self {
            StringOrList::One(value) => value
                .split(',')
                .map(|item| item.trim().trim_start_matches('#').to_string())
                .filter(|item| !item.is_empty())
                .collect(),
            StringOrList::Many(values) => values
                .into_iter()
                .map(|item| item.trim().trim_start_matches('#').to_string())
                .filter(|item| !item.is_empty())
                .collect(),
            StringOrList::Missing => Vec::new(),
        }
    }
}

/// YAML-тело frontmatter: между строками `---` (допускается `...`).
fn parse_frontmatter(block_chars: &[char]) -> Option<ParsedFrontmatter> {
    let block: String = block_chars.iter().collect();
    let mut lines = block.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    let mut body: Vec<&str> = lines.collect();
    if body
        .last()
        .is_some_and(|last| matches!(last.trim(), "---" | "..."))
    {
        body.pop();
    }
    let frontmatter: Frontmatter = serde_yml::from_str(&body.join("\n")).ok()?;
    Some(ParsedFrontmatter {
        title: frontmatter.title,
        aliases: frontmatter.aliases.into_vec(),
        tags: frontmatter.tags.into_vec(),
    })
}

/// Нормализованный frontmatter.
struct ParsedFrontmatter {
    title: Option<String>,
    aliases: Vec<String>,
    tags: Vec<String>,
}

// ── хелперы ──────────────────────────────────────────────────────────────

fn collect_chars(chars: &[char], range: &std::ops::Range<usize>) -> String {
    chars[range.clone()].iter().collect()
}

fn line_of(chars: &[char], offset: usize) -> usize {
    chars[..offset].iter().filter(|c| **c == '\n').count()
}

fn line_text(chars: &[char], offset: usize) -> String {
    let mut start = offset;
    while start > 0 && chars[start - 1] != '\n' {
        start -= 1;
    }
    let mut end = offset;
    while end < chars.len() && chars[end] != '\n' {
        end += 1;
    }
    chars[start..end]
        .iter()
        .collect::<String>()
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fixture(text: &str) -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("Заметка.md"), text).unwrap();
        let vault = Vault::open(dir.path()).unwrap();
        (dir, vault)
    }

    #[test]
    fn frontmatter_list_and_title() {
        let (dir, vault) = fixture(
            "---\ntitle: Тестовая\naliases: [Один, Два]\ntags: [Работа, идея/вход]\n---\n\nтекст\n",
        );
        let meta = parse_note(&vault, &NoteId::new("Заметка.md"));
        assert_eq!(meta.title, "Тестовая");
        assert_eq!(meta.aliases, vec!["Один".to_string(), "Два".to_string()]);
        // теги сортируются и дедуплицируются
        assert_eq!(
            meta.tags,
            vec!["идея/вход".to_string(), "работа".to_string()]
        );
        drop(dir);
    }

    #[test]
    fn frontmatter_comma_string() {
        let (dir, vault) = fixture("---\ntags: Alpha, Beta\n---\n");
        let meta = parse_note(&vault, &NoteId::new("Заметка.md"));
        assert_eq!(meta.tags, vec!["alpha".to_string(), "beta".to_string()]);
        drop(dir);
    }

    #[test]
    fn title_defaults_to_file_stem() {
        let (dir, vault) = fixture("просто текст\n");
        let meta = parse_note(&vault, &NoteId::new("Заметка.md"));
        assert_eq!(meta.title, "Заметка");
        drop(dir);
    }

    #[test]
    fn headings_links_and_tags() {
        let (dir, vault) = fixture(
            "# Основной\n\nсм. [[Другая|в сторону]] и [сайт](https://example.com)\n\n## Под\n\n#тег/раз\n",
        );
        let meta = parse_note(&vault, &NoteId::new("Заметка.md"));
        assert_eq!(meta.headings.len(), 2);
        assert_eq!(meta.headings[0].level, 1);
        assert_eq!(meta.headings[0].text, "Основной");
        assert_eq!(meta.headings[1].text, "Под");

        assert_eq!(meta.links.len(), 2);
        assert_eq!(meta.links[0].target_text, "Другая");
        assert_eq!(meta.links[0].display.as_deref(), Some("в сторону"));
        assert_eq!(meta.links[0].line, 2);
        assert!(meta.links[0].context.contains("см."));
        assert_eq!(meta.links[1].target_text, "https://example.com");
        assert_eq!(meta.links[1].kind, LinkKind::Markdown);

        assert_eq!(meta.tags, vec!["тег/раз".to_string()]);
        drop(dir);
    }

    #[test]
    fn embed_is_link_kind_embed() {
        let (dir, vault) = fixture("![[pic.png]] и [[note]]\n");
        let meta = parse_note(&vault, &NoteId::new("Заметка.md"));
        assert_eq!(meta.links[0].kind, LinkKind::Embed);
        assert_eq!(meta.links[1].kind, LinkKind::Wiki);
        drop(dir);
    }
}
