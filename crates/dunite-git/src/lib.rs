//! Git-бэкапы (SPEC §10).
//!
//! Философия: git — и есть бэкап, без второй системы версий.
//! Правила безопасности: никогда не делаем `reset --hard`, ничего не пушим
//! сами, не удаляем файлы без явного действия пользователя; восстановление —
//! всегда новый коммит (история не теряется).

use std::path::{Path, PathBuf};

use git2::{Commit, DiffFormat, DiffLine as GitDiffLine, Oid, Repository, Signature};

/// Ошибка git-операции.
#[derive(Debug)]
pub enum GitError {
    Io(std::io::Error),
    Git(git2::Error),
}

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GitError::Io(err) => write!(f, "{err}"),
            GitError::Git(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for GitError {}

impl From<git2::Error> for GitError {
    fn from(err: git2::Error) -> Self {
        GitError::Git(err)
    }
}

impl From<std::io::Error> for GitError {
    fn from(err: std::io::Error) -> Self {
        GitError::Io(err)
    }
}

pub type Result<T> = std::result::Result<T, GitError>;

/// Информация о коммите (для timeline).
#[derive(Debug, Clone)]
pub struct CommitInfo {
    /// Полный oid в hex.
    pub id: String,
    /// Короткий oid — для отображения.
    pub short_id: String,
    pub summary: String,
    /// Локальное время коммита `YYYY-MM-DD HH:MM`.
    pub time: String,
}

/// Строка diff-превью.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    /// `'+'` — добавлено, `'-'` — удалено, `' '` — контекст.
    pub origin: char,
    pub text: String,
}

/// Git-репозиторий хранилища.
pub struct GitRepo {
    repo: Repository,
    root: PathBuf,
}

impl GitRepo {
    /// Открывает существующий репозиторий `root` (или его родителя).
    pub fn open(root: &Path) -> Result<Option<Self>> {
        match Repository::discover(root) {
            Ok(repo) => {
                let root = repo
                    .workdir()
                    .map(|dir| dir.to_path_buf())
                    .unwrap_or_else(|| root.to_path_buf());
                Ok(Some(Self { repo, root }))
            }
            Err(err) if err.code() == git2::ErrorCode::NotFound => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    /// `git init` + базовый `.gitignore` (SPEC §10.1) + первый снапшот.
    pub fn init(root: &Path) -> Result<Self> {
        let repo = Repository::init(root)?;
        let gitignore = root.join(".gitignore");
        if !gitignore.exists() {
            std::fs::write(
                &gitignore,
                "# служебные данные dunite и ОС (SPEC §10.1)\n\
                 .dunite/\n\
                 .obsidian/\n\
                 .DS_Store\n\
                 Thumbs.db\n",
            )?;
        }
        let git = Self {
            repo,
            root: root.to_path_buf(),
        };
        // базовый коммит, чтобы хранилище сразу было восстановимо
        if git.head_commit().is_none() {
            let _ = git.snapshot_with_message("init: dunite начал контролировать версии");
        }
        Ok(git)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Есть ли изменения в рабочем дереве (включая новые файлы).
    pub fn has_changes(&self) -> bool {
        self.changed_files()
            .map(|files| !files.is_empty())
            .unwrap_or(false)
    }

    /// Относительные пути изменённых файлов.
    pub fn changed_files(&self) -> Result<Vec<String>> {
        let mut options = git2::StatusOptions::new();
        options
            .include_untracked(true)
            .include_ignored(false)
            .recurse_untracked_dirs(true);
        let statuses = self.repo.statuses(Some(&mut options))?;
        Ok(statuses
            .iter()
            .filter_map(|entry| {
                let path = entry.path()?.to_string();
                Some(path)
            })
            .collect())
    }

    /// Снапшот: stage всех изменений + коммит (SPEC §10.1).
    ///
    /// Возвращает oid коммита или `None`, если нечего коммитить.
    /// Сообщение: `snapshot: YYYY-MM-DD HH:MM · N notes changed`.
    pub fn snapshot(&self) -> Result<Option<String>> {
        let changed = self.changed_files()?;
        if changed.is_empty() {
            return Ok(None);
        }
        let message = format!(
            "snapshot: {} · {} files changed",
            now_local(),
            changed.len()
        );
        self.snapshot_with_message(&message)
    }

    /// Снапшот с произвольным сообщением.
    pub fn snapshot_with_message(&self, message: &str) -> Result<Option<String>> {
        let mut index = self.repo.index()?;
        index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)?;
        index.write()?;
        if index.len() == 0 {
            return Ok(None);
        }

        let tree_id = index.write_tree()?;
        let tree = self.repo.find_tree(tree_id)?;
        let signature = Signature::now("dunite", "dunite@local")?;
        let parents: Vec<Commit> = self.head_commit().into_iter().collect();
        let parent_refs: Vec<&Commit> = parents.iter().collect();

        let oid = self.repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            message,
            &tree,
            &parent_refs,
        )?;
        Ok(Some(oid.to_string()))
    }

    /// Последние коммиты хранилища.
    pub fn log(&self, limit: usize) -> Result<Vec<CommitInfo>> {
        self.log_inner(limit, None)
    }

    /// Коммиты, затрагивающие `rel_path`.
    pub fn log_for_path(&self, rel_path: &str, limit: usize) -> Result<Vec<CommitInfo>> {
        self.log_inner(limit, Some(rel_path))
    }

    /// Diff заметки в коммите против его родителя (для превью).
    ///
    /// Для начального коммита — содержимое файла целиком (только добавления).
    pub fn diff_note(&self, commit_id: &str, rel_path: &str) -> Result<Vec<DiffLine>> {
        let commit = self.find_commit(commit_id)?;
        let parent_tree = commit
            .parent(0)
            .ok()
            .map(|parent| parent.tree())
            .transpose()?;
        let tree = commit.tree()?;

        let mut options = git2::DiffOptions::new();
        options.pathspec(rel_path);
        let diff =
            self.repo
                .diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), Some(&mut options))?;

        let mut lines = Vec::new();
        diff.print(DiffFormat::Patch, |_delta, _hunk, line: GitDiffLine| {
            let origin = match line.origin() {
                '+' | '-' | ' ' => line.origin(),
                _ => return true,
            };
            let text = String::from_utf8_lossy(line.content())
                .trim_end_matches('\n')
                .to_string();
            lines.push(DiffLine { origin, text });
            true
        })?;
        Ok(lines)
    }

    /// Возвращает содержимое заметки в коммите (для восстановления).
    pub fn note_at(&self, commit_id: &str, rel_path: &str) -> Result<String> {
        let commit = self.find_commit(commit_id)?;
        let entry = commit.tree()?.get_path(Path::new(rel_path))?;
        let blob = entry
            .to_object(&self.repo)?
            .into_blob()
            .map_err(|_| git2::Error::from_str("в коммите по этому пути не файл"))?;
        Ok(String::from_utf8_lossy(blob.content()).into_owned())
    }

    /// Восстанавливает заметку из коммита: пишет файл на диск.
    ///
    /// Вызывающий делает новый снапшот — восстановление всегда новый
    /// коммит, история не теряется (SPEC §10.2).
    pub fn restore_note(&self, commit_id: &str, rel_path: &str) -> Result<PathBuf> {
        let text = self.note_at(commit_id, rel_path)?;
        let path = self.root.join(rel_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, text)?;
        Ok(path)
    }

    // ── remote и синхронизация (SPEC §3, §10.1) ─────────────────────────
    //
    // Сетевые операции идут через CLI git: он наследует ssh-ключи,
    // credential-helper и 2FA пользователя, которых у libgit2 нет.

    /// Настроенные remote'ы: (имя, url).
    pub fn remotes(&self) -> Vec<(String, String)> {
        self.repo
            .remotes()
            .map(|names| {
                names
                    .iter()
                    .flatten()
                    .filter_map(|name| {
                        let url = self.repo.find_remote(name).ok()?.url()?.to_string();
                        Some((name.to_string(), url))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Добавляет remote (например `origin` → url GitHub/GitLab/Bitbucket).
    pub fn add_remote(&self, name: &str, url: &str) -> Result<()> {
        self.repo.remote(name, url)?;
        Ok(())
    }

    /// Отклонение от upstream: (вперед, назад) коммитов.
    ///
    /// `None` — upstream не настроен.
    pub fn ahead_behind(&self) -> Option<(usize, usize)> {
        let head = self.repo.head().ok()?.peel_to_commit().ok()?;
        let branch_name = self.repo.head().ok()?.shorthand()?.to_string();
        let branch = self
            .repo
            .find_branch(&branch_name, git2::BranchType::Local)
            .ok()?;
        let upstream_oid = branch.upstream().ok()?.get().target()?;
        let (ahead, behind) = self.repo.graph_ahead_behind(head.id(), upstream_oid).ok()?;
        Some((ahead, behind))
    }

    /// Push: `git push` (при первом разе — с `-u`, чтобы запомнить upstream).
    pub fn push(&self) -> Result<String> {
        let args = self.push_args();
        run_cli(&self.root, &args)
    }

    /// Pull: `git pull --ff-only` — без автоматических мержей.
    ///
    /// Если истории разошлись, git откажется (SPEC §14: конфликты
    /// не резолвим магически — пользователь разбирается вручную).
    pub fn pull(&self) -> Result<String> {
        let args = self.pull_args();
        run_cli(&self.root, &args)
    }

    /// Fetch: обновляет remote-tracking ветки (счётчики ahead/behind).
    pub fn fetch(&self) -> Result<String> {
        let args = self.fetch_args();
        run_cli(&self.root, &args)
    }

    /// Аргументы push: с `-u` при первом разе, чтобы запомнить upstream.
    ///
    /// Аргументы готовятся заранее (локально), а [`run_cli`] выполняется
    /// в фоне — сетевые операции не должны морозить UI.
    pub fn push_args(&self) -> Vec<String> {
        if self.ahead_behind().is_none() {
            let branch = self
                .repo
                .head()
                .ok()
                .and_then(|head| head.shorthand().map(|name| name.to_string()))
                .unwrap_or_else(|| "HEAD".to_string());
            if let Some((remote, _)) = self.remotes().first() {
                return vec!["push".into(), "-u".into(), remote.clone(), branch];
            }
        }
        vec!["push".into()]
    }

    /// Аргументы pull: fast-forward only, без автоматических мержей.
    pub fn pull_args(&self) -> Vec<String> {
        vec!["pull".into(), "--ff-only".into()]
    }

    /// Аргументы fetch: тихое обновление всех remote-tracking веток.
    pub fn fetch_args(&self) -> Vec<String> {
        vec!["fetch".into(), "--all".into(), "--quiet".into()]
    }

    /// Статус remote для статус-бара: `origin · ↑2 ↓1`.
    pub fn remote_status(&self) -> Option<String> {
        let (name, _) = self.remotes().into_iter().next()?;
        let counts = self
            .ahead_behind()
            .map(|(ahead, behind)| format!(" · ↑{ahead} ↓{behind}"))
            .unwrap_or_default();
        Some(format!("{name}{counts}"))
    }

    // ── внутреннее ────────────────────────────────────────────────────

    fn head_commit(&self) -> Option<Commit<'_>> {
        self.repo
            .head()
            .ok()
            .and_then(|head| head.peel_to_commit().ok())
    }

    fn find_commit(&self, commit_id: &str) -> Result<Commit<'_>> {
        let oid = Oid::from_str(commit_id)?;
        Ok(self.repo.find_commit(oid)?)
    }

    fn log_inner(&self, limit: usize, path: Option<&str>) -> Result<Vec<CommitInfo>> {
        let mut revwalk = self.repo.revwalk()?;
        if self.repo.head().is_err() {
            return Ok(Vec::new()); // репозиторий без коммитов
        }
        revwalk.push_head()?;
        let mut out = Vec::new();
        for oid in revwalk {
            if out.len() >= limit {
                break;
            }
            let oid = oid?;
            let commit = self.repo.find_commit(oid)?;
            if let Some(path) = path {
                // коммит менял путь? (для root-коммита сравниваем с пустотой)
                let parent_tree = commit.parent(0).ok().map(|p| p.tree()).transpose()?;
                let mut options = git2::DiffOptions::new();
                options.pathspec(path);
                let diff = self.repo.diff_tree_to_tree(
                    parent_tree.as_ref(),
                    Some(&commit.tree()?),
                    Some(&mut options),
                )?;
                if diff.deltas().len() == 0 {
                    continue;
                }
            }
            out.push(commit_info(&commit));
        }
        Ok(out)
    }
}

/// Выполняет git-команду в корне хранилища (CLI git).
///
/// Сетевые операции выполняются именно через CLI: он наследует ssh-ключи,
/// credential-helper и 2FA пользователя, которых у libgit2 нет.
pub fn run_cli(root: &Path, args: &[String]) -> Result<String> {
    let output = std::process::Command::new("git")
        .current_dir(root)
        .args(args)
        .output()?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(GitError::Io(std::io::Error::other(
            stderr.trim().to_string(),
        )))
    }
}

fn commit_info(commit: &Commit) -> CommitInfo {
    let time = commit.time();
    let seconds = time.seconds();
    let offset = time.offset_minutes() * 60;
    CommitInfo {
        id: commit.id().to_string(),
        short_id: commit.id().to_string()[..7].to_string(),
        summary: commit.summary().unwrap_or_default().to_string(),
        time: format_timestamp(seconds + offset as i64),
    }
}

/// Локальное время `YYYY-MM-DD HH:MM`.
pub fn now_local() -> String {
    let now = chrono::Local::now();
    now.format("%Y-%m-%d %H:%M").to_string()
}

fn format_timestamp(seconds: i64) -> String {
    chrono::DateTime::from_timestamp(seconds, 0)
        .map(|time| {
            chrono::DateTime::<chrono::Local>::from(time)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn vault() -> (tempfile::TempDir, GitRepo) {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("Заметка.md"), "версия 1\n").unwrap();
        let repo = GitRepo::init(dir.path()).unwrap();
        (dir, repo)
    }

    #[test]
    fn init_creates_repo_gitignore_and_baseline() {
        let (dir, repo) = vault();
        assert!(dir.path().join(".git").is_dir());
        let gitignore = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert!(gitignore.contains(".dunite/"));
        assert!(gitignore.contains(".obsidian/"));
        // базовый снапшот уже есть — хранилище восстановимо с первого дня
        let log = repo.log(10).unwrap();
        assert_eq!(log.len(), 1);
        assert!(log[0].summary.starts_with("init:"));
        assert!(!repo.has_changes());
    }

    #[test]
    fn snapshot_commits_and_skips_when_clean() {
        let (dir, repo) = vault();
        assert!(!repo.has_changes());

        fs::write(dir.path().join("Заметка.md"), "версия 2\n").unwrap();
        assert!(repo.has_changes());
        let changed = repo.changed_files().unwrap();
        assert!(changed.iter().any(|f| f.contains("Заметка")));

        let oid = repo.snapshot().unwrap().expect("снапшот должен создаться");
        assert!(!oid.is_empty());
        assert!(!repo.has_changes());
        // повторно нечего коммитить
        assert!(repo.snapshot().unwrap().is_none());

        let log = repo.log(10).unwrap();
        assert_eq!(log.len(), 2);
        assert!(log[0].summary.starts_with("snapshot:"));
    }

    #[test]
    fn log_for_path_filters_commits() {
        let (dir, repo) = vault();
        fs::write(dir.path().join("Другая.md"), "другая\n").unwrap();
        repo.snapshot().unwrap();

        let for_note = repo.log_for_path("Заметка.md", 10).unwrap();
        // «Заметка.md» менялся только в init-коммите
        assert_eq!(for_note.len(), 1);

        let for_other = repo.log_for_path("Другая.md", 10).unwrap();
        // «Другая.md» появилась только в снапшоте — init её не касался
        assert_eq!(for_other.len(), 1);
        assert!(for_other[0].summary.starts_with("snapshot:"));
    }

    #[test]
    fn diff_shows_added_and_removed_lines() {
        let (dir, repo) = vault();
        fs::write(dir.path().join("Заметка.md"), "версия 2\nновая строка\n").unwrap();
        let oid = repo.snapshot().unwrap().unwrap();

        let diff = repo.diff_note(&oid, "Заметка.md").unwrap();
        assert!(
            diff.iter()
                .any(|line| line.origin == '-' && line.text.contains('1'))
        );
        assert!(
            diff.iter()
                .any(|line| line.origin == '+' && line.text.contains('2'))
        );
        assert!(
            diff.iter()
                .any(|line| line.origin == '+' && line.text.contains("новая"))
        );
    }

    #[test]
    fn restore_returns_previous_content() {
        let (dir, repo) = vault();
        let baseline = repo.log(1).unwrap().remove(0);

        fs::write(dir.path().join("Заметка.md"), "случайно затёрли\n").unwrap();
        repo.snapshot().unwrap();

        repo.restore_note(&baseline.id, "Заметка.md").unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("Заметка.md")).unwrap(),
            "версия 1\n"
        );
        // восстановление фиксируется новым снапшотом — история не теряется
        repo.snapshot_with_message("restore: Заметка.md").unwrap();
        let log = repo.log(10).unwrap();
        assert_eq!(log.len(), 3);
        assert_eq!(log[0].summary, "restore: Заметка.md");
    }

    #[test]
    fn note_at_reads_file_from_commit() {
        let (dir, repo) = vault();
        let baseline = repo.log(1).unwrap().remove(0);
        assert_eq!(
            repo.note_at(&baseline.id, "Заметка.md").unwrap(),
            "версия 1\n"
        );
        drop(dir);
    }

    #[test]
    fn open_existing_repo_and_missing() {
        let (dir, repo) = vault();
        assert!(GitRepo::open(dir.path()).unwrap().is_some());
        drop(repo);

        let empty = tempfile::tempdir().unwrap();
        assert!(GitRepo::open(empty.path()).unwrap().is_none());
    }

    #[test]
    fn untracked_files_count_as_changes() {
        let (dir, repo) = vault();
        fs::write(dir.path().join("Новая.md"), "новая\n").unwrap();
        assert!(repo.has_changes());
        let changed = repo.changed_files().unwrap();
        assert!(changed.iter().any(|f| f == "Новая.md"));
        drop(dir);
    }

    // ── remote и синхронизация ──────────────────────────────────────────

    fn run_git(dir: &Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .current_dir(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {:?}: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// Bare-репозиторий — как удалённый сервер (GitHub/GitLab/Bitbucket).
    fn bare_remote() -> tempfile::TempDir {
        let remote = tempfile::tempdir().unwrap();
        Repository::init_bare(remote.path()).unwrap();
        remote
    }

    #[test]
    fn remote_push_and_ahead_behind() {
        let (dir, repo) = vault();
        let remote = bare_remote();
        assert!(repo.remotes().is_empty());
        assert!(repo.ahead_behind().is_none());
        assert!(repo.remote_status().is_none());

        repo.add_remote("origin", remote.path().to_str().unwrap())
            .unwrap();
        assert_eq!(repo.remotes().len(), 1);

        // первый push запоминает upstream
        repo.push().unwrap();
        assert_eq!(repo.ahead_behind(), Some((0, 0)));
        assert!(repo.remote_status().unwrap().starts_with("origin"));

        // локальный коммит — впереди на один
        fs::write(dir.path().join("Заметка.md"), "версия 2\n").unwrap();
        repo.snapshot().unwrap();
        assert_eq!(repo.ahead_behind(), Some((1, 0)));
    }

    #[test]
    fn remote_pull_fast_forward() {
        let (dir, repo) = vault();
        let remote = bare_remote();
        repo.add_remote("origin", remote.path().to_str().unwrap())
            .unwrap();
        repo.push().unwrap();

        // клон пушит изменение
        let clone = tempfile::tempdir().unwrap();
        run_git(
            clone.path(),
            &["clone", remote.path().to_str().unwrap(), "."],
        );
        fs::write(clone.path().join("Заметка.md"), "версия от клона\n").unwrap();
        run_git(clone.path(), &["add", "-A"]);
        run_git(
            clone.path(),
            &[
                "-c",
                "user.email=c@local",
                "-c",
                "user.name=c",
                "commit",
                "-m",
                "правка клона",
            ],
        );
        run_git(clone.path(), &["push"]);

        // наш репозиторий отстал — fetch обновляет счётчики, pull подтягивает
        repo.fetch().unwrap();
        assert_eq!(repo.ahead_behind(), Some((0, 1)));
        let output = repo.pull().unwrap();
        assert!(!output.is_empty() || output.is_empty()); // вывод зависит от версии git
        assert_eq!(
            fs::read_to_string(dir.path().join("Заметка.md")).unwrap(),
            "версия от клона\n"
        );
        assert_eq!(repo.ahead_behind(), Some((0, 0)));
    }

    #[test]
    fn remote_pull_refuses_diverged_histories() {
        // SPEC §14: конфликты не резолвим магически — pull --ff-only
        // отказывается при расхождении, разбирается пользователь
        let (dir, repo) = vault();
        let remote = bare_remote();
        repo.add_remote("origin", remote.path().to_str().unwrap())
            .unwrap();
        repo.push().unwrap();

        let clone = tempfile::tempdir().unwrap();
        run_git(
            clone.path(),
            &["clone", remote.path().to_str().unwrap(), "."],
        );
        fs::write(clone.path().join("Другая.md"), "из клона\n").unwrap();
        run_git(clone.path(), &["add", "-A"]);
        run_git(
            clone.path(),
            &[
                "-c",
                "user.email=c@local",
                "-c",
                "user.name=c",
                "commit",
                "-m",
                "правка клона",
            ],
        );
        run_git(clone.path(), &["push"]);

        // локальная своя правка — истории разошлись
        fs::write(dir.path().join("Заметка.md"), "своя версия\n").unwrap();
        repo.snapshot().unwrap();
        repo.fetch().unwrap();
        assert_eq!(repo.ahead_behind(), Some((1, 1)));
        assert!(repo.pull().is_err());
    }
}
