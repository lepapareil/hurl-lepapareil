/*
 * Hurl (https://hurl.dev)
 * Copyright (C) 2026 Orange
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *          http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 *
 */
//! File system watcher: tells the GUI that a collection changed on disk.

use notify::event::ModifyKind;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::time::Duration;

use crate::collection::{LEGACY_ENVIRONMENTS_DIR, OPTIONS_EXTENSION, is_ignored_name};

const DEBOUNCE: Duration = Duration::from_millis(250);

fn has_extension(path: &Path, extension: &str) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(extension))
}

/// Does a change of `path` affect what the GUI shows of the collection
/// `root`? The rules mirror the scan of the collection: a build writing in
/// `target/`, an editor saving a source file or git updating `.git/` must not
/// cost a refresh.
fn relevant(root: &Path, path: &Path, kind: &EventKind) -> bool {
    let Ok(rel) = path.strip_prefix(root) else {
        // Reported under another spelling of the root (links): when in doubt,
        // refresh.
        return true;
    };
    let ignored = rel.components().any(|c| match c {
        Component::Normal(name) => is_ignored_name(&name.to_string_lossy()),
        _ => false,
    });
    if ignored {
        return false;
    }
    // Removals and renames may concern a folder: its former kind is unknown.
    if matches!(
        kind,
        EventKind::Remove(_) | EventKind::Modify(ModifyKind::Name(_))
    ) {
        return true;
    }
    // Environments are the `.env` files at the root of the collection (and
    // the ones former versions kept in their own directory).
    let environment = has_extension(rel, "env")
        && (rel.components().count() == 1 || rel.starts_with(LEGACY_ENVIRONMENTS_DIR));
    has_extension(rel, "hurl")
        || has_extension(rel, OPTIONS_EXTENSION)
        || environment
        || path.is_dir()
}

/// Watches `path` recursively and calls `on_change` once per burst of events.
/// The watch stops when the returned watcher is dropped.
pub fn watch(
    path: &Path,
    on_change: impl Fn() + Send + 'static,
) -> Result<RecommendedWatcher, String> {
    let (tx, rx) = channel::<()>();
    // Some backends report canonical paths.
    let roots: Vec<PathBuf> = [Some(path.to_path_buf()), std::fs::canonicalize(path).ok()]
        .into_iter()
        .flatten()
        .collect();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
        if let Ok(event) = res {
            if matches!(event.kind, EventKind::Access(_)) {
                return;
            }
            let concerned = event.paths.is_empty()
                || event.paths.iter().any(|p| {
                    let root = roots.iter().find(|r| p.starts_with(r)).unwrap_or(&roots[0]);
                    relevant(root, p, &event.kind)
                });
            if concerned {
                let _ = tx.send(());
            }
        }
    })
    .map_err(|e| e.to_string())?;
    watcher
        .watch(path, RecursiveMode::Recursive)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    std::thread::spawn(move || {
        while rx.recv().is_ok() {
            loop {
                match rx.recv_timeout(DEBOUNCE) {
                    Ok(()) => continue,
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
            on_change();
        }
    });
    Ok(watcher)
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{CreateKind, DataChange, RemoveKind, RenameMode};

    #[test]
    fn only_what_the_collection_shows_is_relevant() {
        // The collection itself may live under an ignored name.
        let root = Path::new("/home/me/.config/node_modules/api");
        let write = EventKind::Modify(ModifyKind::Data(DataChange::Content));
        let create = EventKind::Create(CreateKind::File);
        let remove = EventKind::Remove(RemoveKind::Any);
        let rename = EventKind::Modify(ModifyKind::Name(RenameMode::Any));
        let at = |rel: &str| root.join(rel);

        assert!(relevant(root, &at("users/list.hurl"), &write));
        assert!(relevant(root, &at("new.HURL"), &create));
        assert!(relevant(root, &at("local.env"), &write));
        assert!(relevant(root, &at("users/users.options"), &write));
        assert!(relevant(root, &at("users/list.hurl.options"), &create));
        assert!(relevant(root, &at("environments/local.env"), &write));
        assert!(relevant(root, &at("users"), &remove));
        assert!(relevant(root, &at("old-name"), &rename));

        assert!(!relevant(root, &at("src/main.rs"), &write));
        assert!(!relevant(root, &at("README.md"), &create));
        assert!(!relevant(root, &at("users/.env"), &write));
        assert!(!relevant(root, &at("users/local.env"), &write));
        assert!(!relevant(root, &at("target/debug/build.hurl"), &write));
        assert!(!relevant(root, &at(".git/index"), &rename));
        assert!(!relevant(root, &at("node_modules/x/y.hurl"), &remove));
        assert!(!relevant(root, &at(".idea/workspace.xml"), &write));

        // Not under the root as we know it: refresh rather than miss a change.
        assert!(relevant(root, Path::new("/elsewhere/a.txt"), &write));
    }
}
