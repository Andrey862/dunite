//! Полнотекстовый поиск (SPEC §11).
//!
//! In-memory индекс: постинги (токен → заметки с частотами) строятся тем же
//! конвейером разбора, что и граф ссылок. Скоринг — BM25; язык запросов —
//! слова (AND), «фразы», фильтры `tag:`, `path:`, `link:`.

use dunite_vault::NoteId;

/// Разобранный поисковый запрос.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SearchQuery {
    /// Слова: все должны встречаться (AND).
    pub terms: Vec<String>,
    /// Точные фразы в кавычках: все должны встречаться (AND).
    pub phrases: Vec<String>,
    /// Фильтр `tag:work` — тег или его подтеги.
    pub tags: Vec<String>,
    /// Фильтр `path:projects/` — подстрока пути.
    pub paths: Vec<String>,
    /// Фильтр `link:Alpha` — заметки, ссылающиеся на цель.
    pub links: Vec<String>,
}

impl SearchQuery {
    /// Пустой запрос (только фильтры) — допустим.
    pub fn is_empty(&self) -> bool {
        self.terms.is_empty() && self.phrases.is_empty()
    }

    /// Все «иглы» для подсветки: фразы и слова.
    pub fn needles(&self) -> Vec<String> {
        self.phrases
            .iter()
            .chain(self.terms.iter())
            .cloned()
            .collect()
    }
}

/// Максимум результатов поиска (SPEC §11): сниппеты строятся только для
/// топа — иначе широкие запросы на больших хранилищах съедают память.
pub const MAX_RESULTS: usize = 50;

/// Разбирает строку запроса.
///
/// `"точная фраза"` — фраза; `tag:work`, `path:projects/`, `link:Alpha` —
/// фильтры (поддерживают кавычки: `tag:"мой тег"`); остальное — слова.
pub fn parse_query(input: &str) -> SearchQuery {
    let mut query = SearchQuery::default();
    for token in tokenize_query(input) {
        if let Some((prefix, rest)) = token.split_once(':') {
            if matches!(prefix, "tag" | "path" | "link") && !rest.is_empty() {
                let value = rest.trim_matches('"').to_lowercase();
                match prefix {
                    "tag" => query.tags.push(value),
                    "path" => query.paths.push(value),
                    _ => query.links.push(
                        value
                            .trim_start_matches("[[")
                            .trim_end_matches("]]")
                            .to_string(),
                    ),
                }
                continue;
            }
        }
        if token.starts_with('"') && token.ends_with('"') && token.len() >= 2 {
            // пустая фраза («""») не добавляется — иначе пустая
            // «игла» зависает в построении сниппетов
            let phrase = token.trim_matches('"').to_lowercase();
            if !phrase.is_empty() {
                query.phrases.push(phrase);
            }
        } else if !token.is_empty() {
            query.terms.push(token.to_lowercase());
        }
    }
    query
}

/// Разбиение запроса на токены с учётом кавычек.
fn tokenize_query(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    for ch in input.chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                current.push(ch);
            }
            c if c.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// Токенизация текста: юникод-последовательности букв/цифр в нижнем регистре.
pub fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(|token| token.to_lowercase())
        .collect()
}

/// Сниппет: строка заметки с диапазонами совпадений.
#[derive(Debug, Clone)]
pub struct Snippet {
    /// Номер строки (0-based) — для перехода.
    pub line: usize,
    pub text: String,
    /// Byte-диапазоны совпадений внутри `text`.
    pub ranges: Vec<(usize, usize)>,
}

/// Результат поиска.
#[derive(Debug, Clone)]
pub struct SearchHit {
    pub id: NoteId,
    pub score: f32,
    pub snippets: Vec<Snippet>,
}

/// Максимум сниппетов на заметку (SPEC §11).
pub const MAX_SNIPPETS: usize = 3;

/// Строит сниппеты по телу заметки: до [`MAX_SNIPPETS`] строк с совпадениями.
pub fn build_snippets(body: &str, needles: &[String]) -> Vec<Snippet> {
    let mut snippets = Vec::new();
    for (line_ix, line) in body.lines().enumerate() {
        if snippets.len() >= MAX_SNIPPETS {
            break;
        }
        let lower = line.to_lowercase();
        let mut ranges = Vec::new();
        for needle in needles {
            if needle.is_empty() {
                // защита от зависания: find("") всегда совпадает
                continue;
            }
            let mut from = 0;
            while let Some(found) = lower[from..].find(needle.as_str()) {
                let start = from + found;
                let end = start + needle.len();
                ranges.push((start, end));
                from = end;
            }
        }
        if ranges.is_empty() {
            continue;
        }
        ranges.sort();
        // слить пересекающиеся диапазоны
        let mut merged: Vec<(usize, usize)> = Vec::new();
        for range in ranges {
            match merged.last_mut() {
                Some(last) if range.0 <= last.1 => last.1 = last.1.max(range.1),
                _ => merged.push(range),
            }
        }
        snippets.push(Snippet {
            line: line_ix,
            text: line.to_string(),
            ranges: merged,
        });
    }
    snippets
}

/// BM25-подобный вес одного термина: `idf * tf / (tf + 1.2)`.
pub fn term_score(tf: u32, df: usize, total_docs: usize) -> f32 {
    let n = total_docs.max(1) as f32;
    let df = df.max(1) as f32;
    let idf = (1. + (n - df + 0.5) / (df + 0.5)).ln();
    idf * (tf as f32) / (tf as f32 + 1.2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_mixed_query() {
        let query = parse_query(r#"слова "точная фраза" tag:work path:projects/ link:Alpha"#);
        assert_eq!(query.terms, vec!["слова".to_string()]);
        assert_eq!(query.phrases, vec!["точная фраза".to_string()]);
        assert_eq!(query.tags, vec!["work".to_string()]);
        assert_eq!(query.paths, vec!["projects/".to_string()]);
        assert_eq!(query.links, vec!["alpha".to_string()]);
    }

    #[test]
    fn parse_link_brackets_stripped() {
        let query = parse_query("link:[[Бета]]");
        assert_eq!(query.links, vec!["бета".to_string()]);
    }

    #[test]
    fn parse_quoted_tag() {
        let query = parse_query(r#"tag:"мой тег" и слово"#);
        assert_eq!(query.tags, vec!["мой тег".to_string()]);
        assert_eq!(query.terms, vec!["и".to_string(), "слово".to_string()]);
    }

    #[test]
    fn tokenize_unicode() {
        assert_eq!(
            tokenize("Hello, Привет-Мир 42!"),
            vec!["hello", "привет", "мир", "42"]
        );
    }

    #[test]
    fn empty_query_pieces_are_ignored() {
        // регрессия: «""» порождало пустую фразу и зависание в сниппетах
        let query = parse_query("\"\" \"");
        assert!(query.phrases.is_empty());
        assert_eq!(query.terms, vec!["\"".to_string()]);

        // пустая игла не зацикливает построение сниппетов
        let snippets = build_snippets("любой текст", &["".to_string(), "текст".to_string()]);
        assert_eq!(snippets.len(), 1);
    }

    #[test]
    fn snippets_mark_matches_and_merge() {
        let snippets = build_snippets("один два\nтри один", &["один".to_string()]);
        assert_eq!(snippets.len(), 2);
        assert_eq!(snippets[0].line, 0);
        // диапазоны — byte-смещения: «один» = 8 байт
        assert_eq!(snippets[0].ranges, vec![(0, 8)]);
        assert_eq!(snippets[1].line, 1);
        assert_eq!(snippets[1].ranges, vec![(7, 15)]);
    }

    #[test]
    fn snippets_cap_at_three() {
        let body = "совпадение\n".repeat(10);
        let snippets = build_snippets(&body, &["совпадение".to_string()]);
        assert_eq!(snippets.len(), MAX_SNIPPETS);
    }

    #[test]
    fn higher_tf_scores_higher() {
        let low = term_score(1, 1, 100);
        let high = term_score(5, 1, 100);
        assert!(high > low, "{high} vs {low}");
    }

    #[test]
    fn rarer_terms_score_higher() {
        let rare = term_score(1, 1, 100);
        let common = term_score(1, 80, 100);
        assert!(rare > common, "{rare} vs {common}");
    }
}
