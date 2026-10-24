//! Глобальные настройки dunite (SPEC §12).
//!
//! Глобальный конфиг: `~/.config/dunite/settings.toml`.
//! Отсутствие или порча файла — валидное состояние: используются дефолты.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Встроенная тема оформления (SPEC §12: ровно две темы).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Theme {
    #[default]
    Light,
    Dark,
}

impl Theme {
    pub fn name(self) -> &'static str {
        match self {
            Theme::Light => "light",
            Theme::Dark => "dark",
        }
    }
}

/// Глобальные настройки приложения.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Тема оформления.
    pub theme: Theme,
    /// Недавно открытые хранилища (последнее — первым, максимум [`Settings::MAX_RECENT`]).
    pub recent_vaults: Vec<PathBuf>,
    /// Задержка автоснапшота git после последнего изменения, сек.
    /// `0` — автоснапшоты выключены (SPEC §10.1).
    pub snapshot_debounce_secs: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::default(),
            recent_vaults: Vec::new(),
            snapshot_debounce_secs: 60,
        }
    }
}

impl Settings {
    const MAX_RECENT: usize = 10;

    /// Путь к глобальному файлу настроек, `None` если XDG-каталог недоступен.
    pub fn config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|dir| dir.join("dunite").join("settings.toml"))
    }

    /// Загрузка настроек; любая ошибка чтения/парсинга → дефолты.
    pub fn load() -> Self {
        let Some(path) = Self::config_path() else {
            return Self::default();
        };
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Сохранение настроек; создаёт каталог при необходимости.
    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = Self::config_path() else {
            return Ok(());
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = toml::to_string_pretty(self).expect("settings must serialize");
        std::fs::write(path, text)
    }

    /// Запомнить хранилище в списке недавних (без дублей, максимум [`Self::MAX_RECENT`]).
    pub fn touch_vault(&mut self, root: &Path) {
        self.recent_vaults.retain(|p| p != root);
        self.recent_vaults.insert(0, root.to_path_buf());
        self.recent_vaults.truncate(Self::MAX_RECENT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touch_vault_dedupes_and_caps() {
        let mut settings = Settings::default();
        for i in 0..15 {
            settings.touch_vault(Path::new(&format!("/vault{i}")));
        }
        assert_eq!(settings.recent_vaults.len(), Settings::MAX_RECENT);
        assert_eq!(settings.recent_vaults[0], PathBuf::from("/vault14"));

        settings.touch_vault(Path::new("/vault5"));
        assert_eq!(settings.recent_vaults.len(), Settings::MAX_RECENT);
        assert_eq!(settings.recent_vaults[0], PathBuf::from("/vault5"));
        assert_eq!(
            settings
                .recent_vaults
                .iter()
                .filter(|p| *p == Path::new("/vault5"))
                .count(),
            1
        );
    }

    #[test]
    fn roundtrip_toml() {
        let mut settings = Settings::default();
        settings.theme = Theme::Dark;
        settings.touch_vault(Path::new("/notes"));
        let text = toml::to_string_pretty(&settings).unwrap();
        let parsed: Settings = toml::from_str(&text).unwrap();
        assert_eq!(parsed.theme, Theme::Dark);
        assert_eq!(parsed.recent_vaults, vec![PathBuf::from("/notes")]);
    }
}
