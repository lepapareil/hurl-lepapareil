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
//! Collections on disk.
//!
//! A collection is a plain directory: every `*.hurl` file is a request file
//! (one or several entries), sub-directories are folders and the `<name>.env`
//! files at its root are its environments: Hurl variables files
//! (`name=value`).
//!
//! A folder and a Hurl file can have options of `hurl` of their own, in an
//! options file: `<folder>/<folder name>.options` for a folder,
//! `<file name>.options` next to a Hurl file (`users.hurl.options`). See
//! [`crate::options::parse_file`] for its format.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::fs::{self, File, Metadata};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::hurlfile::{self, Entry, KeyValue};
use crate::options;
use crate::store;

const MAX_DEPTH: usize = 20;
const MAX_PARSED_SIZE: u64 = 2 * 1024 * 1024;
/// Largest file opened in the GUI.
const MAX_TEXT_SIZE: u64 = 20 * 1024 * 1024;
/// Directory of a collection where former versions kept its environments.
/// The ones still there are listed, and moved to the root of the collection
/// when they are saved or renamed.
pub const LEGACY_ENVIRONMENTS_DIR: &str = "environments";

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct EntrySummary {
    pub index: usize,
    pub method: String,
    pub url: String,
    pub name: String,
}

#[derive(Serialize, Debug)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Node {
    Folder {
        name: String,
        path: String,
        /// Options of its options file, by long name.
        options: Map<String, Value>,
        children: Vec<Node>,
    },
    File {
        name: String,
        path: String,
        /// Changes when the file does (see [`file_stamp`]).
        stamp: String,
        /// Options of its options file, by long name.
        options: Map<String, Value>,
        entries: Vec<EntrySummary>,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Variable {
    pub name: String,
    pub value: String,
    pub secret: bool,
}

#[derive(Serialize, Debug)]
pub struct Environment {
    pub name: String,
    pub path: String,
    pub variables: Vec<Variable>,
}

#[derive(Serialize, Debug)]
pub struct Collection {
    pub path: String,
    pub name: String,
    /// Options of its options file, by long name.
    pub options: Map<String, Value>,
    pub children: Vec<Node>,
    pub environments: Vec<Environment>,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct HurlFile {
    pub path: String,
    /// Stamp of the file when it was read (see [`file_stamp`]).
    pub stamp: String,
    pub raw: String,
    pub entries: Vec<Entry>,
    /// Message of `hurlfmt` when the file does not parse.
    pub syntax_error: Option<String>,
}

fn display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn io_err(path: &Path, e: std::io::Error) -> String {
    format!("{}: {e}", path.display())
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| display(path))
}

/// Extension of an options file.
pub const OPTIONS_EXTENSION: &str = "options";

fn is_options(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == OPTIONS_EXTENSION)
}

/// Options file of a folder (`<folder>/<folder name>.options`) or of a Hurl
/// file (`<file name>.options`, next to it).
pub fn options_path(path: &str) -> PathBuf {
    let path = Path::new(path);
    let name = format!("{}.{OPTIONS_EXTENSION}", file_name(path));
    if path.is_dir() {
        path.join(name)
    } else {
        path.with_file_name(name)
    }
}

fn read_options_file(path: &Path) -> Map<String, Value> {
    read_regular(path, MAX_PARSED_SIZE)
        .map(|text| options::parse_file(&text).0)
        .unwrap_or_default()
}

/// Writes the options of a folder or of a Hurl file in its options file.
/// The lines of the file which are not options set here (comments...) are
/// kept; the file is removed when nothing is left to write in it.
pub fn save_options(path: &str, values: &Map<String, Value>) -> Result<(), String> {
    if !Path::new(path).exists() {
        return Err(format!("{path} does not exist"));
    }
    let file = options_path(path);
    let others = match read_regular(&file, MAX_PARSED_SIZE) {
        Ok(text) => options::parse_file(&text).1,
        Err(_) => vec![],
    };
    let text = options::format_file(values, &others)?;
    if text.is_empty() {
        return match fs::remove_file(&file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(io_err(&file, e)),
            _ => Ok(()),
        };
    }
    fs::write(&file, text).map_err(|e| io_err(&file, e))
}

fn is_hurl(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("hurl"))
}

/// Files and directories which are never part of a collection. Shared by the
/// scan and by the watcher, so that what is not shown does not trigger a
/// refresh either.
pub fn is_ignored_name(name: &str) -> bool {
    name.starts_with('.') || name == "node_modules" || name == "target"
}

/// A modification time that close to the current time is not trusted: see
/// [`file_stamp`]. Larger than the coarsest time stamps of file systems (2
/// seconds on FAT).
const RACY_WINDOW: Duration = Duration::from_secs(3);

/// Stamp of a file, and whether a later equal stamp means "same content".
struct Stamp {
    text: String,
    stable: bool,
}

impl Stamp {
    fn of(path: &Path) -> Stamp {
        match fs::metadata(path) {
            Ok(meta) => Stamp::at(&meta, SystemTime::now()),
            // An empty stamp says nothing about the file.
            Err(_) => Stamp {
                text: String::new(),
                stable: false,
            },
        }
    }

    /// Stamp of a file whose metadata were read at the time `now`.
    fn at(meta: &Metadata, now: SystemTime) -> Stamp {
        static FRESH: AtomicU64 = AtomicU64::new(0);
        let modified = meta.modified().ok();
        let nanos = |t: SystemTime| t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let text = format!("{}-{}", modified.map_or(0, nanos), meta.len());
        // Stable when the file was last modified long enough ago: also false
        // for a time in the future or which is not known.
        let stable = modified
            .and_then(|m| now.duration_since(m).ok())
            .is_some_and(|age| age >= RACY_WINDOW);
        if stable {
            return Stamp { text, stable };
        }
        // The counter tells apart two stamps taken within one tick of the
        // clock.
        let unique = FRESH.fetch_add(1, Ordering::Relaxed);
        Stamp {
            text: format!("{text}-fresh{}.{unique}", nanos(now)),
            stable,
        }
    }
}

/// Cheap identity of the content of a file: `<modification time>-<size>`.
/// Lets the GUI reload, and the collection scan parse, only the files which
/// changed.
///
/// Time stamps of file systems may be coarse (1 second, 2 on FAT): two edits
/// of the same size made within one tick cannot be told apart, and a stamp
/// taken between them would wrongly vouch for the content written by the
/// second one. So a file modified less than `RACY_WINDOW` ago gets a
/// `-fresh<time>.<counter>` suffix which makes its stamp unique: it never
/// equals a stamp taken later, the file is read again on every refresh until
/// its stamp settles, and once more then. A stamp without that suffix was
/// taken when no further write could share its modification time (as long as
/// the file system and this machine agree on the time: a file server whose
/// clock is seconds late defeats the rule, as it does for `git` or `make`).
///
/// The stamp must be taken *before* reading the content it stands for: when
/// the file changes in between, the stamp is the outdated one and the file is
/// read again.
pub fn file_stamp(path: &Path) -> String {
    Stamp::of(path).text
}

/// Reads a text file of at most `limit` bytes. Only regular files are read
/// (symbolic links are followed): a link to a device or a named pipe would
/// otherwise be read without end.
fn read_regular(path: &Path, limit: u64) -> Result<String, String> {
    let meta = fs::metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("not a regular file".into());
    }
    if meta.len() > limit {
        return Err(format!("larger than {} MB", limit / 1024 / 1024));
    }
    let mut text = String::new();
    File::open(path)
        .map_err(|e| e.to_string())?
        .take(limit)
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    Ok(text)
}

struct Scan {
    nodes: Vec<Node>,
    hurl_files: usize,
    files: usize,
    /// Options of the directory itself.
    options: Map<String, Value>,
}

struct CachedFile {
    /// Always a stable stamp (see [`file_stamp`]).
    stamp: String,
    entries: Vec<EntrySummary>,
}

/// Entry summaries of the Hurl files of one collection, kept from one scan
/// to the next: a collection is scanned after every change on disk, and
/// reading and parsing all its files again is what a scan costs.
#[derive(Default)]
pub struct ScanCache {
    files: HashMap<PathBuf, CachedFile>,
}

/// Cache during a scan: what the previous scan knew, and what this one saw.
/// Only the latter is kept, so that deleted files leave the cache.
struct ScanPass {
    previous: HashMap<PathBuf, CachedFile>,
    seen: HashMap<PathBuf, CachedFile>,
}

impl ScanPass {
    /// Entry summaries of a Hurl file, read from the file unless its stamp
    /// shows that it did not change since they were cached.
    fn summaries(&mut self, path: &Path, stamp: &Stamp) -> Vec<EntrySummary> {
        // A stamp which is not stable never matches: nothing is cached under
        // such a stamp.
        let unchanged = |cached: &CachedFile| stamp.stable && cached.stamp == stamp.text;
        if let Some(cached) = self.previous.remove(path).filter(unchanged) {
            let entries = cached.entries.clone();
            self.seen.insert(path.to_path_buf(), cached);
            return entries;
        }
        let Ok(text) = read_regular(path, MAX_PARSED_SIZE) else {
            // Not cached: the reason may go away without the stamp changing
            // (access rights).
            return vec![];
        };
        let entries = summaries(&hurlfile::parse(&text));
        if stamp.stable {
            let cached = CachedFile {
                stamp: stamp.text.clone(),
                entries: entries.clone(),
            };
            self.seen.insert(path.to_path_buf(), cached);
        }
        entries
    }
}

fn scan_dir(dir: &Path, depth: usize, pass: &mut ScanPass) -> Scan {
    let mut scan = Scan {
        nodes: vec![],
        hurl_files: 0,
        files: 0,
        options: Map::new(),
    };
    let Ok(read) = fs::read_dir(dir) else {
        return scan;
    };
    let mut folders = vec![];
    let mut files = vec![];
    // Options files of the directory, by name.
    let mut option_files: HashMap<String, PathBuf> = HashMap::new();
    for entry in read.flatten() {
        let path = entry.path();
        let name = file_name(&path);
        if is_ignored_name(&name) {
            continue;
        }
        // Symbolic links to directories are not followed (no cycles).
        match entry.file_type() {
            Ok(t) if t.is_dir() => {
                if depth < MAX_DEPTH {
                    let sub = scan_dir(&path, depth + 1, pass);
                    scan.hurl_files += sub.hurl_files;
                    scan.files += sub.files;
                    // Folders of an unrelated project are hidden; empty
                    // folders stay visible so that they can be filled.
                    if sub.hurl_files > 0 || sub.files == 0 {
                        folders.push(Node::Folder {
                            name,
                            path: display(&path),
                            options: sub.options,
                            children: sub.nodes,
                        });
                    }
                }
            }
            // Not a file of an unrelated project: it does not hide a folder.
            Ok(_) if is_options(&path) => {
                option_files.insert(name, path);
            }
            Ok(_) => {
                scan.files += 1;
                if is_hurl(&path) {
                    scan.hurl_files += 1;
                    // Taken before the file is read (see `file_stamp`).
                    let stamp = Stamp::of(&path);
                    let entries = pass.summaries(&path, &stamp);
                    files.push(Node::File {
                        name,
                        stamp: stamp.text,
                        path: display(&path),
                        options: Map::new(),
                        entries,
                    });
                }
            }
            Err(_) => {}
        }
    }
    let key = |n: &Node| match n {
        Node::Folder { name, .. } | Node::File { name, .. } => name.to_lowercase(),
    };
    for file in &mut files {
        if let Node::File { name, options, .. } = file
            && let Some(path) = option_files.get(&format!("{name}.{OPTIONS_EXTENSION}"))
        {
            *options = read_options_file(path);
        }
    }
    if let Some(path) = option_files.get(&format!("{}.{OPTIONS_EXTENSION}", file_name(dir))) {
        scan.options = read_options_file(path);
    }
    folders.sort_by_key(key);
    files.sort_by_key(key);
    scan.nodes = folders;
    scan.nodes.extend(files);
    scan
}

fn summaries(entries: &[Entry]) -> Vec<EntrySummary> {
    entries
        .iter()
        .map(|e| EntrySummary {
            index: e.index,
            method: e.method.clone(),
            url: e.url.clone(),
            name: e.name(),
        })
        .collect()
}

/// Scans the collection at `path`. `cache` is the one of the previous scan
/// of this collection (an empty one for a first scan): files which did not
/// change since are not read again. It is left with the files of this scan.
pub fn scan(config_dir: &Path, path: &str, cache: &mut ScanCache) -> Result<Collection, String> {
    let root = Path::new(path);
    let mut pass = ScanPass {
        previous: std::mem::take(&mut cache.files),
        seen: HashMap::new(),
    };
    if !root.is_dir() {
        // Nothing is kept of a collection which is gone.
        return Err(format!("{path} is not a directory"));
    }
    let scanned = scan_dir(root, 0, &mut pass);
    cache.files = pass.seen;
    Ok(Collection {
        path: path.to_string(),
        name: file_name(root),
        options: scanned.options,
        children: scanned.nodes,
        environments: environments(config_dir, path),
    })
}

// --- Environments -----------------------------------------------------------

/// File of an environment: `<name>.env`, at the root of the collection.
pub fn environment_path(collection: &str, name: &str) -> PathBuf {
    Path::new(collection).join(format!("{name}.env"))
}

fn legacy_environment_path(collection: &str, name: &str) -> PathBuf {
    Path::new(collection)
        .join(LEGACY_ENVIRONMENTS_DIR)
        .join(format!("{name}.env"))
}

/// File of an environment as it is on disk: at the root of the collection,
/// or still where former versions kept it.
pub fn environment_file(collection: &str, name: &str) -> PathBuf {
    let path = environment_path(collection, name);
    let legacy = legacy_environment_path(collection, name);
    if !path.is_file() && legacy.is_file() {
        legacy
    } else {
        path
    }
}

/// Removes the directory of former versions once nothing is left in it.
fn remove_empty_legacy_dir(collection: &str) {
    // Fails, as wanted, when the directory is not empty.
    let _ = fs::remove_dir(Path::new(collection).join(LEGACY_ENVIRONMENTS_DIR));
}

/// The `<name>.env` files of a directory. Hidden files (`.env`, which is not
/// a Hurl variables file by convention) are left out.
fn environment_files(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(read) = fs::read_dir(dir) else {
        return vec![];
    };
    read.flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == "env"))
        .filter_map(|path| {
            let name = path.file_stem()?.to_string_lossy().into_owned();
            (!name.starts_with('.')).then_some((name, path))
        })
        .collect()
}

/// Hurl variables file: `name=value`, blank lines and `#` comments ignored.
pub fn parse_variables(text: &str) -> Vec<KeyValue> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let (name, value) = line.split_once('=')?;
            Some(KeyValue {
                name: name.trim().to_string(),
                value: value.trim().to_string(),
            })
        })
        .collect()
}

pub fn environments(config_dir: &Path, collection: &str) -> Vec<Environment> {
    let root = Path::new(collection);
    let mut files = environment_files(root);
    // An environment of a former version is hidden by one of the same name
    // at the root.
    for (name, path) in environment_files(&root.join(LEGACY_ENVIRONMENTS_DIR)) {
        if !files.iter().any(|(n, _)| *n == name) {
            files.push((name, path));
        }
    }
    let mut envs: Vec<Environment> = files
        .into_iter()
        .filter_map(|(name, path)| {
            let text = read_regular(&path, MAX_PARSED_SIZE).ok()?;
            let mut variables: Vec<Variable> = parse_variables(&text)
                .into_iter()
                .map(|kv| Variable {
                    name: kv.name,
                    value: kv.value,
                    secret: false,
                })
                .collect();
            variables.extend(
                store::secrets(config_dir, collection, &name)
                    .into_iter()
                    .map(|kv| Variable {
                        name: kv.name,
                        value: kv.value,
                        secret: true,
                    }),
            );
            Some(Environment {
                name,
                path: display(&path),
                variables,
            })
        })
        .collect();
    envs.sort_by_key(|e| e.name.to_lowercase());
    envs
}

/// Creates an empty environment. Unlike [`save_environment`], refuses a name
/// which is already used (also when it only differs by case: the file would
/// be the same on Windows and macOS).
pub fn create_environment(config_dir: &Path, collection: &str, name: &str) -> Result<(), String> {
    check_name(name)?;
    let exists = environment_path(collection, name).exists()
        || environments(config_dir, collection)
            .iter()
            .any(|e| e.name.to_lowercase() == name.to_lowercase());
    if exists {
        return Err(format!("environment {name} already exists"));
    }
    save_environment(config_dir, collection, name, &[])
}

pub fn save_environment(
    config_dir: &Path,
    collection: &str,
    name: &str,
    variables: &[Variable],
) -> Result<(), String> {
    check_name(name)?;
    store::check_secrets(config_dir)?;
    let mut text = String::new();
    let mut secrets = vec![];
    for v in variables {
        let var_name = v.name.trim();
        if var_name.is_empty() {
            continue;
        }
        if var_name.contains('=') || var_name.starts_with('#') {
            return Err(format!("invalid variable name: {var_name}"));
        }
        if v.value.contains('\n') {
            return Err(format!(
                "variable {var_name}: multiline values are not supported"
            ));
        }
        if v.secret {
            secrets.push(KeyValue {
                name: var_name.to_string(),
                value: v.value.clone(),
            });
        } else {
            text.push_str(&format!("{var_name}={}\n", v.value.trim()));
        }
    }
    let path = environment_path(collection, name);
    fs::write(&path, text).map_err(|e| io_err(&path, e))?;
    // Saved at the root: the file of a former version is replaced by it.
    let legacy = legacy_environment_path(collection, name);
    if legacy.is_file() {
        fs::remove_file(&legacy).map_err(|e| io_err(&legacy, e))?;
        remove_empty_legacy_dir(collection);
    }
    store::set_secrets(config_dir, collection, name, Some(&secrets))
}

pub fn delete_environment(config_dir: &Path, collection: &str, name: &str) -> Result<(), String> {
    check_name(name)?;
    store::check_secrets(config_dir)?;
    let path = environment_file(collection, name);
    fs::remove_file(&path).map_err(|e| io_err(&path, e))?;
    remove_empty_legacy_dir(collection);
    store::set_secrets(config_dir, collection, name, None)
}

pub fn rename_environment(
    config_dir: &Path,
    collection: &str,
    name: &str,
    new_name: &str,
) -> Result<(), String> {
    check_name(name)?;
    check_name(new_name)?;
    store::check_secrets(config_dir)?;
    let from = environment_file(collection, name);
    let to = environment_path(collection, new_name);
    if to.exists() || environment_file(collection, new_name).exists() {
        return Err(format!("environment {new_name} already exists"));
    }
    fs::rename(&from, &to).map_err(|e| io_err(&from, e))?;
    remove_empty_legacy_dir(collection);
    store::rename_secrets(config_dir, collection, name, new_name)
}

// --- Files and folders ------------------------------------------------------

/// A single path component, valid on every platform.
pub fn check_name(name: &str) -> Result<(), String> {
    let invalid = name.trim().is_empty()
        || name == "."
        || name == ".."
        || name.ends_with(['.', ' '])
        || name
            .chars()
            .any(|c| c.is_control() || "/\\<>:\"|?*".contains(c));
    if invalid {
        Err(format!("invalid name: {name:?}"))
    } else {
        Ok(())
    }
}

fn new_child(parent: &str, name: &str) -> Result<PathBuf, String> {
    check_name(name)?;
    let parent = Path::new(parent);
    if !parent.is_dir() {
        return Err(format!("{} is not a directory", parent.display()));
    }
    let path = parent.join(name);
    if path.exists() {
        return Err(format!("{} already exists", path.display()));
    }
    Ok(path)
}

fn with_hurl_extension(name: &str) -> String {
    if name.to_lowercase().ends_with(".hurl") {
        name.to_string()
    } else {
        format!("{name}.hurl")
    }
}

pub fn create_collection(parent: &str, name: &str) -> Result<String, String> {
    check_name(name)?;
    let path = Path::new(parent).join(name);
    if path.exists()
        && fs::read_dir(&path)
            .map_err(|e| io_err(&path, e))?
            .next()
            .is_some()
    {
        return Err(format!(
            "{} already exists and is not empty",
            path.display()
        ));
    }
    fs::create_dir_all(&path).map_err(|e| io_err(&path, e))?;
    Ok(display(&path))
}

pub fn create_folder(parent: &str, name: &str) -> Result<String, String> {
    let path = new_child(parent, name)?;
    fs::create_dir(&path).map_err(|e| io_err(&path, e))?;
    Ok(display(&path))
}

pub fn create_file(parent: &str, name: &str, entry: Option<&Entry>) -> Result<String, String> {
    let path = new_child(parent, &with_hurl_extension(name))?;
    let text = match entry {
        Some(e) => hurlfile::checked_entry(e, "\n")?,
        None => String::new(),
    };
    fs::write(&path, text).map_err(|e| io_err(&path, e))?;
    Ok(display(&path))
}

pub fn rename_path(path: &str, name: &str) -> Result<String, String> {
    let from = Path::new(path);
    let name = if from.is_file() && is_hurl(from) {
        with_hurl_extension(name)
    } else {
        name.to_string()
    };
    let parent = from
        .parent()
        .ok_or_else(|| format!("{path} has no parent directory"))?;
    let to = new_child(&display(parent), &name)?;
    // Its options file is named after it: taken before the name changes.
    let options_from = options_path(path);
    fs::rename(from, &to).map_err(|e| io_err(from, e))?;
    // The options follow: the file of a folder is now in the renamed folder.
    let options_from = if to.is_dir() {
        to.join(file_name(&options_from))
    } else {
        options_from
    };
    let options_to = options_path(&display(&to));
    if options_from.is_file() && !options_to.exists() {
        fs::rename(&options_from, &options_to).map_err(|e| io_err(&options_from, e))?;
    }
    Ok(display(&to))
}

pub fn clone_file(path: &str, name: &str) -> Result<String, String> {
    let from = Path::new(path);
    if !from.is_file() {
        return Err(format!("{path} is not a file"));
    }
    let parent = from
        .parent()
        .ok_or_else(|| format!("{path} has no parent directory"))?;
    let to = new_child(&display(parent), &with_hurl_extension(name))?;
    fs::copy(from, &to).map_err(|e| io_err(from, e))?;
    // A clone runs as its model: with the same options.
    let options_from = options_path(path);
    let options_to = options_path(&display(&to));
    if options_from.is_file() && !options_to.exists() {
        fs::copy(&options_from, &options_to).map_err(|e| io_err(&options_from, e))?;
    }
    Ok(display(&to))
}

pub fn delete_path(path: &str) -> Result<(), String> {
    let p = Path::new(path);
    let meta = fs::symlink_metadata(p).map_err(|e| io_err(p, e))?;
    if meta.is_dir() {
        fs::remove_dir_all(p).map_err(|e| io_err(p, e))
    } else {
        // The options of a file go with it.
        let options = options_path(path);
        fs::remove_file(p).map_err(|e| io_err(p, e))?;
        if options.is_file() {
            fs::remove_file(&options).map_err(|e| io_err(&options, e))?;
        }
        Ok(())
    }
}

pub fn read_text(path: &str) -> Result<String, String> {
    read_regular(Path::new(path), MAX_TEXT_SIZE).map_err(|e| format!("{path}: {e}"))
}

/// Path of `target` as it must be written in a `file,...;` body of
/// `hurl_file`: relative to the directory of the Hurl file (the default file
/// root of Hurl), `/` separated, with the escapes of Hurl file names.
/// The flag tells that `target` is outside of that directory: Hurl refuses
/// such a file unless `--file-root` allows it.
pub fn body_file_path(hurl_file: &str, target: &str) -> (String, bool) {
    let base = Path::new(hurl_file).parent().unwrap_or(Path::new(""));
    let (path, outside) = match Path::new(target).strip_prefix(base) {
        Ok(rel) if !rel.as_os_str().is_empty() => (rel.to_string_lossy().into_owned(), false),
        _ => (target.to_string(), true),
    };
    let path = if cfg!(windows) {
        path.replace('\\', "/")
    } else {
        path
    };
    let mut escaped = String::with_capacity(path.len());
    for c in path.chars() {
        if " #;{}:\\".contains(c) {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    (escaped, outside)
}

pub fn write_text(path: &str, text: &str) -> Result<(), String> {
    fs::write(path, text).map_err(|e| format!("{path}: {e}"))
}

/// Opens the system file manager on `path`.
pub fn reveal(path: &str) -> Result<(), String> {
    use std::process::Command;
    let p = Path::new(path);
    if !p.exists() {
        return Err(format!("{path} does not exist"));
    }
    let result = if cfg!(target_os = "windows") {
        Command::new("explorer")
            .arg(format!("/select,{path}"))
            .spawn()
    } else if cfg!(target_os = "macos") {
        Command::new("open").arg("-R").arg(p).spawn()
    } else {
        let dir = if p.is_dir() {
            p
        } else {
            p.parent().unwrap_or(p)
        };
        Command::new("xdg-open").arg(dir).spawn()
    };
    result
        .map(|_| ())
        .map_err(|e| format!("cannot open the file manager: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "hurlui-test-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn names() {
        for ok in ["users", "get user.hurl", "été", "a.b"] {
            assert!(check_name(ok).is_ok(), "{ok}");
        }
        for bad in [
            "", " ", ".", "..", "a/b", "a\\b", "a:b", "a?", "end.", "a\nb",
        ] {
            assert!(check_name(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn body_file_paths() {
        let hurl = if cfg!(windows) {
            "C:\\api\\users\\a.hurl"
        } else {
            "/api/users/a.hurl"
        };
        let inside = if cfg!(windows) {
            "C:\\api\\users\\data\\my file#1.bin"
        } else {
            "/api/users/data/my file#1.bin"
        };
        assert_eq!(
            body_file_path(hurl, inside),
            ("data/my\\ file\\#1.bin".to_string(), false)
        );
        let outside = if cfg!(windows) {
            "D:\\x.bin"
        } else {
            "/other/x.bin"
        };
        let (path, is_outside) = body_file_path(hurl, outside);
        assert!(is_outside);
        assert!(path.ends_with("x.bin"));
        // Read back by the Hurl file model without loss.
        let text = format!("POST http://a\nfile,{};\n", "data/my\\ file\\#1.bin");
        let entry = &hurlfile::parse(&text)[0];
        assert_eq!(
            (entry.body.mode.as_str(), entry.body.content.as_str()),
            ("file", "data/my\\ file\\#1.bin")
        );
        assert_eq!(hurlfile::serialize_entry(entry, "\n"), text);
    }

    #[cfg(unix)]
    #[test]
    fn scan_only_reads_regular_files() {
        let root = temp_dir("special");
        let config = root.join("config");
        let col = create_collection(&display(&root), "api").unwrap();
        // Would be read without end if the size check looked at the link.
        std::os::unix::fs::symlink("/dev/zero", Path::new(&col).join("zero.hurl")).unwrap();
        create_file(
            &col,
            "ok",
            Some(&Entry {
                url: "http://a".into(),
                ..Entry::default()
            }),
        )
        .unwrap();
        let scanned = scan(&config, &col, &mut ScanCache::default()).unwrap();
        let entries: Vec<(String, usize)> = scanned
            .children
            .iter()
            .map(|n| match n {
                Node::File { name, entries, .. } => (name.clone(), entries.len()),
                Node::Folder { name, .. } => (name.clone(), 0),
            })
            .collect();
        assert_eq!(
            entries,
            [("ok.hurl".to_string(), 1), ("zero.hurl".to_string(), 0)]
        );
        assert!(read_text(&display(&Path::new(&col).join("zero.hurl"))).is_err());
        fs::remove_dir_all(&root).unwrap();
    }

    fn set_modified(path: &Path, time: SystemTime) {
        File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(time)
            .unwrap();
    }

    /// `(url of the first entry, stamp)` of every file at the root.
    fn first_urls(collection: &Collection) -> Vec<(String, String)> {
        collection
            .children
            .iter()
            .filter_map(|n| match n {
                Node::File { entries, stamp, .. } => Some((
                    entries.first().map(|e| e.url.clone()).unwrap_or_default(),
                    stamp.clone(),
                )),
                Node::Folder { .. } => None,
            })
            .collect()
    }

    fn cached_names(cache: &ScanCache) -> Vec<String> {
        let mut names: Vec<String> = cache.files.keys().map(|p| file_name(p)).collect();
        names.sort();
        names
    }

    #[test]
    fn stamps_of_recent_files_are_unique() {
        let root = temp_dir("stamp");
        let file = root.join("a.hurl");
        fs::write(&file, "GET http://a\n").unwrap();
        let meta = fs::metadata(&file).unwrap();
        let modified = meta.modified().unwrap();
        let nanos = modified.duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let after = |millis: u64| modified + Duration::from_millis(millis);

        // Old enough: the stamp only depends on the file.
        let stable = Stamp::at(&meta, after(3000));
        assert!(stable.stable);
        assert_eq!(stable.text, format!("{nanos}-13"));
        assert_eq!(Stamp::at(&meta, after(86_400_000)).text, stable.text);
        // Too recent, or modified "in the future": never equal to another stamp.
        for now in [after(0), after(2999), modified - Duration::from_secs(3600)] {
            let (one, two) = (Stamp::at(&meta, now), Stamp::at(&meta, now));
            assert!(!one.stable && !two.stable);
            assert!(
                one.text.starts_with(&format!("{nanos}-13-fresh")),
                "{}",
                one.text
            );
            assert_ne!(one.text, two.text);
            assert_ne!(one.text, stable.text);
        }

        // Through the file system.
        set_modified(&file, SystemTime::now() + Duration::from_secs(3600));
        assert_ne!(file_stamp(&file), file_stamp(&file));
        set_modified(&file, SystemTime::now() - Duration::from_secs(3600));
        assert_eq!(file_stamp(&file), file_stamp(&file));
        assert!(!file_stamp(&file).contains("fresh"));
        assert_eq!(file_stamp(&root.join("missing.hurl")), "");
        assert!(!Stamp::of(&root.join("missing.hurl")).stable);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn unchanged_files_are_not_read_again() {
        let root = temp_dir("cache");
        let config = root.join("config");
        let col = create_collection(&display(&root), "api").unwrap();
        let entry = Entry {
            url: "http://one".into(),
            ..Entry::default()
        };
        let file = PathBuf::from(create_file(&col, "a", Some(&entry)).unwrap());
        let old = SystemTime::now() - Duration::from_secs(3600);
        set_modified(&file, old);

        let mut cache = ScanCache::default();
        let first = first_urls(&scan(&config, &col, &mut cache).unwrap());
        assert_eq!(first[0].0, "http://one");
        assert!(!first[0].1.contains("fresh"), "{}", first[0].1);
        assert_eq!(cached_names(&cache), ["a.hurl"]);

        // Another content with the same size and modification time: only
        // reading the file would tell. The summary comes from the cache.
        fs::write(&file, "GET http://two\n").unwrap();
        set_modified(&file, old);
        assert_eq!(first_urls(&scan(&config, &col, &mut cache).unwrap()), first);
        // Without the cache the file is read.
        let uncached = first_urls(&scan(&config, &col, &mut ScanCache::default()).unwrap());
        assert_eq!(uncached, [("http://two".to_string(), first[0].1.clone())]);

        // Another modification time: read again.
        set_modified(&file, old + Duration::from_secs(1));
        let third = first_urls(&scan(&config, &col, &mut cache).unwrap());
        assert_eq!(third[0].0, "http://two");
        assert_ne!(third[0].1, first[0].1);
        // Another size: read again.
        fs::write(&file, "GET http://three\n").unwrap();
        set_modified(&file, old + Duration::from_secs(1));
        let fourth = first_urls(&scan(&config, &col, &mut cache).unwrap());
        assert_eq!(fourth[0].0, "http://three");
        assert_ne!(fourth[0].1, third[0].1);
        assert_eq!(
            first_urls(&scan(&config, &col, &mut cache).unwrap()),
            fourth
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn recently_modified_files_are_always_read() {
        let root = temp_dir("fresh");
        let config = root.join("config");
        let col = create_collection(&display(&root), "api").unwrap();
        let started = std::time::Instant::now();
        let entry = Entry {
            url: "http://one".into(),
            ..Entry::default()
        };
        let file = PathBuf::from(create_file(&col, "a", Some(&entry)).unwrap());
        let modified = fs::metadata(&file).unwrap().modified().unwrap();

        let mut cache = ScanCache::default();
        let first = first_urls(&scan(&config, &col, &mut cache).unwrap());
        let cached_first = cached_names(&cache);
        // A second edit of the same size within the same tick of a coarse
        // clock: same modification time, same size.
        fs::write(&file, "GET http://two\n").unwrap();
        set_modified(&file, modified);
        let second = first_urls(&scan(&config, &col, &mut cache).unwrap());
        let cached_second = cached_names(&cache);
        if started.elapsed() >= Duration::from_secs(2) {
            // The machine stalled long enough for the file not to be recent.
            eprintln!("too slow: test skipped");
        } else {
            assert_eq!(first[0].0, "http://one");
            assert_eq!(second[0].0, "http://two");
            assert!(first[0].1.contains("-fresh") && second[0].1.contains("-fresh"));
            assert_ne!(first[0].1, second[0].1);
            assert!(cached_first.is_empty() && cached_second.is_empty());
        }

        // Same scenario with a modification time ahead of the clock, which
        // is never trusted either (and does not depend on timing).
        let ahead = SystemTime::now() + Duration::from_secs(3600);
        set_modified(&file, ahead);
        assert_eq!(
            first_urls(&scan(&config, &col, &mut cache).unwrap())[0].0,
            "http://two"
        );
        fs::write(&file, "GET http://six\n").unwrap();
        set_modified(&file, ahead);
        assert_eq!(
            first_urls(&scan(&config, &col, &mut cache).unwrap())[0].0,
            "http://six"
        );
        assert!(cache.files.is_empty());

        // Once the stamp is stable the file is read one last time.
        set_modified(&file, SystemTime::now() - Duration::from_secs(3600));
        let settled = first_urls(&scan(&config, &col, &mut cache).unwrap());
        assert_eq!(settled[0].0, "http://six");
        assert!(!settled[0].1.contains("fresh"));
        assert_eq!(cached_names(&cache), ["a.hurl"]);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn files_which_are_gone_leave_the_cache() {
        let root = temp_dir("evict");
        let config = root.join("config");
        let col = create_collection(&display(&root), "api").unwrap();
        let folder = create_folder(&col, "users").unwrap();
        let entry = Entry {
            url: "http://a".into(),
            ..Entry::default()
        };
        let a = PathBuf::from(create_file(&col, "a", Some(&entry)).unwrap());
        let b = PathBuf::from(create_file(&folder, "b", Some(&entry)).unwrap());
        // Not readable as text: nothing to cache.
        let binary = Path::new(&col).join("binary.hurl");
        fs::write(&binary, [0xff, 0xfe, 0x00]).unwrap();
        let old = SystemTime::now() - Duration::from_secs(3600);
        for file in [&a, &b, &binary] {
            set_modified(file, old);
        }

        let mut cache = ScanCache::default();
        scan(&config, &col, &mut cache).unwrap();
        assert_eq!(cached_names(&cache), ["a.hurl", "b.hurl"]);
        // Deleted, renamed (a rename keeps the modification time).
        fs::remove_file(&b).unwrap();
        fs::rename(&a, a.with_file_name("c.hurl")).unwrap();
        scan(&config, &col, &mut cache).unwrap();
        assert_eq!(cached_names(&cache), ["c.hurl"]);
        fs::remove_dir_all(&folder).unwrap();
        fs::remove_file(a.with_file_name("c.hurl")).unwrap();
        scan(&config, &col, &mut cache).unwrap();
        assert!(cache.files.is_empty());

        // The collection itself is gone.
        create_file(&col, "d", Some(&entry)).unwrap();
        set_modified(&Path::new(&col).join("d.hurl"), old);
        scan(&config, &col, &mut cache).unwrap();
        assert_eq!(cached_names(&cache), ["d.hurl"]);
        fs::remove_dir_all(&col).unwrap();
        assert!(scan(&config, &col, &mut cache).is_err());
        assert!(cache.files.is_empty());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn collection_lifecycle() {
        let root = temp_dir("collection");
        let config = root.join("config");
        let col = create_collection(&display(&root), "api").unwrap();

        let folder = create_folder(&col, "users").unwrap();
        let entry = Entry {
            docs: "List users".into(),
            url: "{{base_url}}/users".into(),
            ..Entry::default()
        };
        let file = create_file(&folder, "list", Some(&entry)).unwrap();
        assert!(file.ends_with("list.hurl"));
        assert!(create_file(&folder, "list.hurl", None).is_err());
        // Hidden content: unrelated sources, dot directories.
        fs::create_dir_all(Path::new(&col).join("src/deep")).unwrap();
        fs::write(Path::new(&col).join("src/deep/main.rs"), "").unwrap();
        fs::create_dir_all(Path::new(&col).join(".git")).unwrap();
        create_folder(&col, "empty").unwrap();

        save_environment(
            &config,
            &col,
            "local",
            &[
                Variable {
                    name: "base_url".into(),
                    value: "http://localhost:8000".into(),
                    secret: false,
                },
                Variable {
                    name: "token".into(),
                    value: "s3cret".into(),
                    secret: true,
                },
                Variable::default(),
            ],
        )
        .unwrap();
        // At the root of the collection.
        assert_eq!(
            environment_path(&col, "local"),
            Path::new(&col).join("local.env")
        );
        let env_text = fs::read_to_string(environment_path(&col, "local")).unwrap();
        assert_eq!(env_text, "base_url=http://localhost:8000\n");
        // Not an environment: a hidden file, a file of a sub-folder.
        fs::write(Path::new(&col).join(".env"), "A=1\n").unwrap();
        fs::write(Path::new(&col).join(".hidden.env"), "A=1\n").unwrap();
        fs::write(Path::new(&folder).join("other.env"), "A=1\n").unwrap();
        // Creating an environment never replaces an existing one.
        assert!(create_environment(&config, &col, "local").is_err());
        assert!(create_environment(&config, &col, "LOCAL").is_err());
        assert_eq!(
            fs::read_to_string(environment_path(&col, "local")).unwrap(),
            env_text
        );
        assert_eq!(store::secrets(&config, &col, "local").len(), 1);
        create_environment(&config, &col, "staging").unwrap();
        delete_environment(&config, &col, "staging").unwrap();

        let scanned = scan(&config, &col, &mut ScanCache::default()).unwrap();
        assert_eq!(scanned.name, "api");
        let names: Vec<String> = scanned
            .children
            .iter()
            .map(|n| match n {
                Node::Folder { name, .. } | Node::File { name, .. } => name.clone(),
            })
            .collect();
        assert_eq!(names, ["empty", "users"]);
        let Node::Folder { children, .. } = &scanned.children[1] else {
            panic!("folder expected")
        };
        let Node::File { entries, .. } = &children[0] else {
            panic!("file expected")
        };
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "List users");
        assert_eq!(scanned.environments.len(), 1);
        assert_eq!(
            scanned.environments[0].variables,
            [
                Variable {
                    name: "base_url".into(),
                    value: "http://localhost:8000".into(),
                    secret: false
                },
                Variable {
                    name: "token".into(),
                    value: "s3cret".into(),
                    secret: true
                },
            ]
        );

        rename_environment(&config, &col, "local", "dev").unwrap();
        let envs = environments(&config, &col);
        assert_eq!(envs[0].name, "dev");
        assert_eq!(envs[0].variables.len(), 2);
        delete_environment(&config, &col, "dev").unwrap();
        assert!(environments(&config, &col).is_empty());
        assert!(store::secrets(&config, &col, "dev").is_empty());

        // Environments of a former version, in `environments/`: listed, run
        // from where they are, and moved to the root when they are written.
        let legacy = Path::new(&col).join(LEGACY_ENVIRONMENTS_DIR);
        fs::create_dir(&legacy).unwrap();
        for name in ["old", "kept", "shadowed", "gone"] {
            fs::write(legacy.join(format!("{name}.env")), "host=legacy\n").unwrap();
        }
        fs::write(Path::new(&col).join("shadowed.env"), "host=root\n").unwrap();
        let envs = environments(&config, &col);
        let names: Vec<&str> = envs.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["gone", "kept", "old", "shadowed"]);
        assert_eq!(envs[3].variables[0].value, "root");
        assert_eq!(environment_file(&col, "old"), legacy.join("old.env"));
        assert_eq!(
            environment_file(&col, "shadowed"),
            environment_path(&col, "shadowed")
        );
        assert!(create_environment(&config, &col, "old").is_err());
        assert!(rename_environment(&config, &col, "kept", "old").is_err());
        // Saved: now at the root.
        let variables = envs[2].variables.clone();
        save_environment(&config, &col, "old", &variables).unwrap();
        assert!(!legacy.join("old.env").exists());
        assert_eq!(
            fs::read_to_string(environment_path(&col, "old")).unwrap(),
            "host=legacy\n"
        );
        // Renamed: at the root too.
        rename_environment(&config, &col, "kept", "moved").unwrap();
        assert!(environment_path(&col, "moved").is_file());
        // The directory goes away with its last environment, not before.
        assert!(legacy.is_dir());
        delete_environment(&config, &col, "gone").unwrap();
        assert!(legacy.join("shadowed.env").is_file());
        fs::remove_file(legacy.join("shadowed.env")).unwrap();
        delete_environment(&config, &col, "moved").unwrap();
        assert!(!legacy.exists());
        for name in ["old", "shadowed"] {
            delete_environment(&config, &col, name).unwrap();
        }
        assert!(environments(&config, &col).is_empty());

        // Options of a folder and of a Hurl file: in their options file.
        let object = |value: Value| value.as_object().unwrap().clone();
        assert_eq!(options_path(&col), Path::new(&col).join("api.options"));
        assert_eq!(
            options_path(&folder),
            Path::new(&folder).join("users.options")
        );
        assert_eq!(
            options_path(&file),
            Path::new(&folder).join("list.hurl.options")
        );
        save_options(&col, &object(json!({ "insecure": true }))).unwrap();
        save_options(
            &folder,
            &object(json!({ "header": ["X-A: 1"], "retry": "2" })),
        )
        .unwrap();
        save_options(&file, &object(json!({ "max-time": "30s" }))).unwrap();
        assert_eq!(
            fs::read_to_string(options_path(&folder)).unwrap(),
            "--header X-A: 1\n--retry 2\n"
        );
        assert!(save_options(&file, &object(json!({ "nope": true }))).is_err());
        assert!(save_options(&format!("{folder}/missing.hurl"), &Map::new()).is_err());
        let scanned = scan(&config, &col, &mut ScanCache::default()).unwrap();
        assert_eq!(Value::Object(scanned.options), json!({ "insecure": true }));
        let Node::Folder {
            options, children, ..
        } = &scanned.children[1]
        else {
            panic!("folder expected")
        };
        assert_eq!(options["retry"], "2");
        let Node::File { name, options, .. } = &children[0] else {
            panic!("file expected")
        };
        // Options files are not listed.
        assert_eq!((children.len(), name.as_str()), (1, "list.hurl"));
        assert_eq!(options["max-time"], "30s");
        // Comments of the file are kept, and the file goes with its last line.
        fs::write(options_path(&col), "# Shared\n--insecure\n").unwrap();
        save_options(&col, &object(json!({ "location": true }))).unwrap();
        assert_eq!(
            fs::read_to_string(options_path(&col)).unwrap(),
            "# Shared\n--location\n"
        );
        fs::remove_file(options_path(&col)).unwrap();
        save_options(&col, &object(json!({ "location": true }))).unwrap();
        save_options(&col, &Map::new()).unwrap();
        assert!(!options_path(&col).exists());
        // A folder which only holds its options is still shown.
        save_options(
            &format!("{col}/empty"),
            &object(json!({ "insecure": true })),
        )
        .unwrap();
        let scanned = scan(&config, &col, &mut ScanCache::default()).unwrap();
        assert!(matches!(&scanned.children[0], Node::Folder { name, .. } if name == "empty"));
        save_options(&format!("{col}/empty"), &Map::new()).unwrap();

        // Options follow a rename, a clone and a removal.
        let renamed = rename_path(&file, "all").unwrap();
        assert!(renamed.ends_with("all.hurl"));
        assert!(!Path::new(&folder).join("list.hurl.options").exists());
        assert!(options_path(&renamed).is_file());
        let cloned = clone_file(&renamed, "copy").unwrap();
        assert_eq!(
            fs::read_to_string(options_path(&cloned)).unwrap(),
            "--max-time 30s\n"
        );
        delete_path(&cloned).unwrap();
        assert!(!options_path(&cloned).exists());
        let moved = rename_path(&folder, "people").unwrap();
        assert_eq!(
            options_path(&moved),
            Path::new(&moved).join("people.options")
        );
        assert!(options_path(&moved).is_file());
        assert!(!Path::new(&moved).join("users.options").exists());
        let folder = rename_path(&moved, "users").unwrap();
        let renamed = format!("{folder}/all.hurl");
        save_options(&renamed, &Map::new()).unwrap();
        save_options(&folder, &Map::new()).unwrap();
        let file = rename_path(&renamed, "list").unwrap();

        let renamed = rename_path(&file, "all").unwrap();
        assert!(renamed.ends_with("all.hurl"));
        let cloned = clone_file(&renamed, "copy").unwrap();
        assert_eq!(
            read_text(&cloned).unwrap(),
            "# List users\nGET {{base_url}}/users\n"
        );
        delete_path(&folder).unwrap();
        assert!(!Path::new(&folder).exists());
        fs::remove_dir_all(&root).unwrap();
    }
}
