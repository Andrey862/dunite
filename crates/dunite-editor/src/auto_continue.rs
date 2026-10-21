//! Авто-продолжение списков и цитат по Enter (SPEC §8.3).

/// План для клавиши Enter на строке `line` (содержимое строки без `\n`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnterPlan {
    /// Обычный перевод строки.
    Newline,
    /// Продолжить список/цитату, вставив префикс.
    Continue(String),
    /// Пустой пункт: удалить маркер строки (первые `delete_chars` символов)
    /// и выйти из списка.
    ExitItem { delete_chars: usize },
}

/// Результат разбора строки списка/цитаты.
struct ParsedItem {
    /// Префикс, которым продолжается список на следующей строке.
    continue_prefix: String,
    /// Длина маркера текущей строки в символах (для выхода из списка).
    marker_chars: usize,
    /// Пустой пункт (содержимое после маркера отсутствует).
    empty: bool,
}

/// Определяет поведение Enter для строки.
pub fn plan_enter(line: &str) -> EnterPlan {
    let parsed = parse_bullet(line)
        .or_else(|| parse_ordered(line))
        .or_else(|| parse_quote(line));
    match parsed {
        Some(item) if item.empty => EnterPlan::ExitItem {
            delete_chars: item.marker_chars,
        },
        Some(item) => EnterPlan::Continue(item.continue_prefix),
        None => EnterPlan::Newline,
    }
}

/// Маркированный список: `[-*+]`, опционально чекбокс `[ ]`/`[x]`.
fn parse_bullet(line: &str) -> Option<ParsedItem> {
    let indent_len = line.len() - line.trim_start().len();
    let rest = &line[indent_len..];
    let marker = rest.chars().next()?;
    if !matches!(marker, '-' | '*' | '+') {
        return None;
    }
    let after_marker = &rest[marker.len_utf8()..];
    if !after_marker.starts_with(' ') {
        return None;
    }
    let spaces_len = after_marker.len() - after_marker.trim_start().len();
    let mut content = &after_marker[spaces_len..];

    // опциональный чекбокс; продолжение всегда начинает незакрытый пункт
    let mut checkbox = String::new();
    if content.starts_with("[ ] ") {
        checkbox = "[ ] ".to_string();
        content = &content[4..];
    } else if content.starts_with("[x] ") || content.starts_with("[X] ") {
        checkbox = "[ ] ".to_string();
        content = &content[4..];
    }

    Some(ParsedItem {
        continue_prefix: format!(
            "{}{}{}{}",
            &line[..indent_len],
            marker,
            &after_marker[..spaces_len],
            checkbox
        ),
        marker_chars: line.chars().count() - content.chars().count(),
        empty: content.trim().is_empty(),
    })
}

/// Нумерованный список: `1.` / `1)`.
fn parse_ordered(line: &str) -> Option<ParsedItem> {
    let indent_len = line.len() - line.trim_start().len();
    let rest = &line[indent_len..];
    let digits_len = rest.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits_len == 0 {
        return None;
    }
    let after_digits = &rest[digits_len..];
    let sep = after_digits.chars().next()?;
    if sep != '.' && sep != ')' {
        return None;
    }
    let after_sep = &after_digits[sep.len_utf8()..];
    if !after_sep.starts_with(' ') {
        return None;
    }
    let spaces_len = after_sep.len() - after_sep.trim_start().len();
    let content = &after_sep[spaces_len..];

    let number: u64 = rest[..digits_len].parse().ok()?;
    Some(ParsedItem {
        continue_prefix: format!(
            "{}{}{}{}",
            &line[..indent_len],
            number + 1,
            sep,
            &after_sep[..spaces_len]
        ),
        marker_chars: line.chars().count() - content.chars().count(),
        empty: content.trim().is_empty(),
    })
}

/// Цитата: `>` с опциональным пробелом.
fn parse_quote(line: &str) -> Option<ParsedItem> {
    let indent_len = line.len() - line.trim_start().len();
    let rest = &line[indent_len..];
    if !rest.starts_with('>') {
        return None;
    }
    let after = &rest[1..];
    if !after.starts_with(' ') {
        return Some(ParsedItem {
            continue_prefix: format!("{}> ", &line[..indent_len]),
            marker_chars: line.chars().count(),
            empty: true,
        });
    }
    let spaces_len = after.len() - after.trim_start().len();
    let content = &after[spaces_len..];
    Some(ParsedItem {
        continue_prefix: format!("{}>{}", &line[..indent_len], &after[..spaces_len]),
        marker_chars: line.chars().count() - content.chars().count(),
        empty: content.trim().is_empty(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_line() {
        assert_eq!(plan_enter("просто текст"), EnterPlan::Newline);
        assert_eq!(plan_enter(""), EnterPlan::Newline);
    }

    #[test]
    fn bullet_continues() {
        assert_eq!(plan_enter("- пункт"), EnterPlan::Continue("- ".into()));
        assert_eq!(plan_enter("* пункт"), EnterPlan::Continue("* ".into()));
        assert_eq!(
            plan_enter("  - вложенный"),
            EnterPlan::Continue("  - ".into())
        );
        assert_eq!(
            plan_enter("-   много пробелов"),
            EnterPlan::Continue("-   ".into())
        );
    }

    #[test]
    fn bullet_empty_item_exits() {
        assert_eq!(plan_enter("- "), EnterPlan::ExitItem { delete_chars: 2 });
        assert_eq!(plan_enter("  * "), EnterPlan::ExitItem { delete_chars: 4 });
    }

    #[test]
    fn checkbox_continues_unchecked() {
        assert_eq!(
            plan_enter("- [ ] задача"),
            EnterPlan::Continue("- [ ] ".into())
        );
        assert_eq!(
            plan_enter("- [x] сделано"),
            EnterPlan::Continue("- [ ] ".into())
        );
    }

    #[test]
    fn checkbox_empty_item_exits() {
        assert_eq!(
            plan_enter("- [ ] "),
            EnterPlan::ExitItem { delete_chars: 6 }
        );
    }

    #[test]
    fn ordered_increments() {
        assert_eq!(plan_enter("1. один"), EnterPlan::Continue("2. ".into()));
        assert_eq!(plan_enter("  3) три"), EnterPlan::Continue("  4) ".into()));
        assert_eq!(plan_enter("10. десять"), EnterPlan::Continue("11. ".into()));
    }

    #[test]
    fn ordered_empty_item_exits_with_current_marker_len() {
        // "9. " — маркер строки из 3 символов, хотя продолжение "10. " — 4
        assert_eq!(plan_enter("9. "), EnterPlan::ExitItem { delete_chars: 3 });
        assert_eq!(plan_enter("5. "), EnterPlan::ExitItem { delete_chars: 3 });
    }

    #[test]
    fn quote_continues() {
        assert_eq!(plan_enter("> цитата"), EnterPlan::Continue("> ".into()));
        assert_eq!(
            plan_enter(" > вложенная"),
            EnterPlan::Continue(" > ".into())
        );
    }

    #[test]
    fn quote_empty_exits() {
        assert_eq!(plan_enter("> "), EnterPlan::ExitItem { delete_chars: 2 });
    }
}
