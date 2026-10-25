# dunite

[Русский](README.md)

> **Authorship.** This project was written entirely by AI — vibe coding with
> the MiMo-V2.6 Pro model in [opencode](https://opencode.ai). Development
> cost: **$4.65** (openrouter).

A high-performance Obsidian clone in Rust + [gpui](https://gpui.rs)
(the UI framework of the Zed IDE). No Electron, no web technologies.

Specification and roadmap: [SPEC.md](SPEC.md) (in Russian). Instructions for
AI agents and contributors: [AGENTS.md](AGENTS.md).

## Features

- Live Preview markdown editor (source stays round-trip safe, render is WYSIWYG)
- File tree, tabs
- Tags, backlinks, images
- Full-text search across the vault
- Git backups: autosnapshots, history, restore

Status: **M0–M6 complete** (roadmap — SPEC.md §16).

## Build and run

```sh
scripts/build-release.sh              # release build (LTO thin, strip)
./target/release/dunite [vault]       # run; defaults to the current directory
```

Flags: `dunite --version`, `dunite --help`.

**Requirements:** Linux (Wayland/X11), a GPU with Vulkan support, `git`
(for backups and sync). Fonts are taken from the system.

Test vault: `testdata/vault-small` (edge-case fixtures —
`ignored/testdata/vault-small/edge-cases` if you set up the playground).

Quality checks: `cargo test` (unit + e2e), `cargo bench` (core),
`scripts/bench_runtime.sh 10000` (budgets in §13, see docs/BENCH.md).

## User guide

**Layout.** The file tree is on the left (context menu: create note/folder,
rename, move, copy/cut/paste, delete; files can be dragged and dropped).
Tabs and the editor are in the center. The right sidebar holds "Outline",
"Backlinks", "Tags" and "History (git)". The status bar at the bottom shows
the cursor position and action messages.

**Global keys:**

| Keys | Action |
|---|---|
| `Ctrl+P` | quick switcher (jump to note) |
| `Ctrl+Shift+F` | search the vault |
| `F2` | rename the current note (links get updated) |
| `Ctrl+S` | save and take a git snapshot |
| `F5` | reload from disk (edits made outside dunite) |
| `Ctrl+G` | create a git repository in the vault |
| `Ctrl+W` | close tab |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | next / previous tab |
| `Alt+←` / `Alt+→` | navigate back / forward |
| `Ctrl+Q` | quit |

**Editor:**

| Keys | Action |
|---|---|
| `←` `→` `↑` `↓`, `Home`/`End`, `PgUp`/`PgDn` | move cursor |
| `Ctrl+←` / `Ctrl+→` | move by word |
| `Ctrl+Home` / `Ctrl+End` | go to start / end of document |
| `Shift+…` | extend selection |
| `Ctrl+A` / `Esc` | select all / clear selection |
| `Ctrl+Z` | undo |
| `Ctrl+Shift+Z` or `Ctrl+Y` | redo |
| `Ctrl+C` / `Ctrl+X` / `Ctrl+V` | copy / cut / paste |
| `Tab` | indent |

In the palette (`Ctrl+P`, `Ctrl+Shift+F`): `Enter` — open, `Esc` — close,
`↑`/`↓` — pick a result.

**Live Preview.** Markup markers (`#`, `**`, `>`…) are concealed while the
cursor is outside the block; selecting or clicking reveals the source.
`[[Links]]` render without brackets, `#tags` and code are highlighted, and
images (`![[file.png]]`, `![](path)`) are shown inline. `Enter` in lists and
quotes continues the item.

**Search.** Substring matches; a "phrase in quotes" matches the exact phrase;
results are the top 50 hits with snippets, `Enter` opens the note.

**Tags and links.** Right sidebar: outline (click — jump to heading),
backlinks (click — open the note), unresolved links, and tags (click — notes
with that tag).

**Git backups.** Autosnapshots are taken after edits; manual — `Ctrl+S` or
the "snapshot" button. In "History (git)": scope chips "Note"/"All", each
commit has a diff preview and "restore …" (always as a new commit). The
`pull`/`push` buttons sync via the git CLI with your own credentials
(`pull` is fast-forward only); `+ remote` connects a remote repository.

**External edits.** dunite watches the vault and picks up external changes
in open tabs; `F5` forces a reload.

## Known limitations

- center-pane splits are a post-MVP task (code lives in git history);
- SVG and PDF are not rendered (SVG is out of MVP scope, PDF is a separate task);
- export to HTML/PDF is out of MVP scope;
- search has no stemming (substring matches, like Obsidian);
- vault indexing happens at startup (~0.9 s for 10k notes), not in the background.

## Structure

Cargo workspace, layer boundaries — SPEC.md §6 (in Russian):

- `crates/dunite-app` — entry point, subsystem wiring
- `crates/dunite-ui` — gpui components
- `crates/dunite-editor` — text engine (ropey)
- `crates/dunite-markdown` — markdown parsing
- `crates/dunite-vault` — vault model
- `crates/dunite-index` — link graph and search
- `crates/dunite-git` — git backups
- `crates/dunite-settings` — settings and themes
