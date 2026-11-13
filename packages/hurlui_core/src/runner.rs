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
//! Runs the `hurl` binary installed on the machine and turns its JSON report
//! into a result usable by the GUI.

use base64::Engine;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::hash_map::RandomState;
use std::collections::{HashMap, VecDeque};
use std::fs::{self, File};
use std::hash::BuildHasher;
use std::io::{ErrorKind, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use crate::collection;
use crate::hurlfile::KeyValue;
use crate::options;

/// Bodies above this size are truncated before being sent to the GUI.
const MAX_BODY_SIZE: usize = 20 * 1024 * 1024;
/// Bytes of response bodies sent to the GUI for one run. The limit on each
/// body is not enough: a file may hold any number of requests with a large
/// response. Bodies beyond this budget are described by their size only.
const MAX_RUN_BODIES: usize = 5 * MAX_BODY_SIZE;
/// Binary bodies are only sent when they can be previewed (images).
const MAX_IMAGE_SIZE: usize = 5 * 1024 * 1024;
/// Bytes kept from each end of the output of `hurl`.
const OUTPUT_HALF: usize = 1024 * 1024;

#[derive(Deserialize, Debug, Default, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct RunRequest {
    pub run_id: String,
    pub file: String,
    pub collection: String,
    pub environment: Option<String>,
    pub from_entry: Option<u32>,
    pub to_entry: Option<u32>,
    /// Explicit path of the `hurl` binary; searched when empty.
    pub hurl_path: String,
    /// Options of `hurl` set in the preferences, by long name (see
    /// [`options::arguments`]).
    pub options: Map<String, Value>,
    /// `none`, `verbose` or `very-verbose`.
    pub verbosity: String,
    /// Extra command line arguments, whitespace separated.
    pub extra_args: String,
}

/// Cancellation flags of the runs in progress.
#[derive(Default)]
pub struct Runs(Mutex<HashMap<String, Arc<AtomicBool>>>);

impl Runs {
    fn register(&self, id: &str) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        self.0.lock().unwrap().insert(id.to_string(), flag.clone());
        flag
    }

    fn unregister(&self, id: &str) {
        self.0.lock().unwrap().remove(id);
    }

    pub fn cancel(&self, id: &str) -> bool {
        match self.0.lock().unwrap().get(id) {
            Some(flag) => {
                flag.store(true, Ordering::SeqCst);
                true
            }
            None => false,
        }
    }
}

// --- Locating binaries ------------------------------------------------------

fn exe_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

/// Directories searched after `PATH`: desktop applications are often started
/// with a minimal `PATH` (macOS Finder, Linux launchers).
fn extra_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = vec![];
    let home = home_dir();
    if cfg!(windows) {
        if let Some(h) = &home {
            dirs.push(h.join(".cargo").join("bin"));
            dirs.push(h.join("scoop").join("shims"));
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            dirs.push(
                Path::new(&local)
                    .join("Microsoft")
                    .join("WinGet")
                    .join("Links"),
            );
        }
        if let Some(pf) = std::env::var_os("ProgramFiles") {
            dirs.push(Path::new(&pf).join("hurl"));
        }
        if let Some(pd) = std::env::var_os("ProgramData") {
            dirs.push(Path::new(&pd).join("chocolatey").join("bin"));
        }
    } else {
        for d in [
            "/usr/local/bin",
            "/opt/homebrew/bin",
            "/usr/bin",
            "/opt/local/bin",
            "/snap/bin",
            "/home/linuxbrew/.linuxbrew/bin",
        ] {
            dirs.push(PathBuf::from(d));
        }
        if let Some(h) = &home {
            dirs.push(h.join(".cargo/bin"));
            dirs.push(h.join(".local/bin"));
            dirs.push(h.join("bin"));
        }
    }
    dirs
}

/// Directory of the application. Hurl UI is shipped with Hurl: the `hurl`
/// found next to it is the one whose options it knows (see [`options`]).
fn own_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.parent().map(Path::to_path_buf)
}

/// Finds a binary: explicit path first, then next to the application, then
/// `PATH`, then well-known installation directories.
pub fn locate(name: &str, explicit: &str) -> Option<PathBuf> {
    let explicit = explicit.trim();
    if !explicit.is_empty() {
        let p = PathBuf::from(explicit);
        return p.is_file().then_some(p);
    }
    let exe = exe_name(name);
    let path_dirs = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .unwrap_or_default();
    own_dir()
        .into_iter()
        .chain(path_dirs)
        .chain(extra_dirs())
        .map(|d| d.join(&exe))
        .find(|p| p.is_file())
}

/// `hurlfmt` is shipped next to `hurl`.
pub fn locate_hurlfmt(hurl_path: &str) -> Option<PathBuf> {
    if let Some(hurl) = locate("hurl", hurl_path) {
        let sibling = hurl.with_file_name(exe_name("hurlfmt"));
        if sibling.is_file() {
            return Some(sibling);
        }
    }
    locate("hurlfmt", "")
}

fn command(program: &Path) -> Command {
    let mut cmd = Command::new(program);
    cmd.env("NO_COLOR", "1");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: no console window flashing behind the GUI.
        cmd.creation_flags(0x0800_0000);
    }
    cmd
}

pub fn version(program: &Path) -> Option<String> {
    let out = command(program).arg("--version").output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines().next().map(|l| l.trim().to_string())
}

pub fn app_info(hurl_path: &str) -> Value {
    let hurl = locate("hurl", hurl_path);
    let hurlfmt = locate_hurlfmt(hurl_path);
    json!({
        "hurlPath": hurl.as_ref().map(|p| p.to_string_lossy()),
        "hurlVersion": hurl.as_ref().and_then(|p| version(p)),
        "hurlfmtPath": hurlfmt.as_ref().map(|p| p.to_string_lossy()),
        // Version of Hurl UI, which is the version of Hurl its options come
        // from.
        "version": env!("CARGO_PKG_VERSION"),
        "os": std::env::consts::OS,
    })
}

// --- hurlfmt ----------------------------------------------------------------

fn run_with_stdin(mut cmd: Command, input: &str) -> Result<std::process::Output, String> {
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot start hurlfmt: {e}"))?;
    let mut stdin = child.stdin.take().expect("piped stdin");
    let input = input.to_string();
    // Written from a thread: a large input must not block on a full pipe.
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(input.as_bytes());
    });
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let _ = writer.join();
    Ok(out)
}

/// Syntax check with `hurlfmt`: `Ok(None)` when the text is valid,
/// `Ok(Some(message))` when it is not, `Err` when `hurlfmt` could not be run
/// to its end (which says nothing about the text).
pub fn check_syntax(hurlfmt: &Path, text: &str) -> Result<Option<String>, String> {
    let mut cmd = command(hurlfmt);
    cmd.args(["--no-color", "--out", "json"]);
    let out = run_with_stdin(cmd, text)?;
    if out.status.success() {
        return Ok(None);
    }
    if out.status.code().is_none() {
        // Killed by a signal: not a verdict.
        return Err(format!("hurlfmt was interrupted: {}", out.status));
    }
    let message = String::from_utf8_lossy(&out.stderr).trim().to_string();
    Ok(Some(if message.is_empty() {
        format!("hurlfmt exited with {}", out.status)
    } else {
        message
    }))
}

/// How long a location of `hurlfmt` is trusted. Short, so that Hurl installed
/// or removed while the application runs is noticed without a restart.
const LOCATION_TTL: Duration = Duration::from_secs(30);
/// Number of syntax checks remembered.
const MAX_CHECKS: usize = 64;

/// Identity of a syntax check: hash of the `hurlfmt` path and of the text,
/// and length of the text.
type CheckKey = (u64, usize);

/// What is remembered about `hurlfmt` between two commands.
///
/// Reading a file would otherwise search the `PATH` twice and start a
/// process each time, even for a content which was just checked: the GUI
/// reads the files shown in tabs again whenever their collection changes.
#[derive(Default)]
pub struct Hurlfmt {
    /// Location (if any) for each `hurlPath` preference, and when it was
    /// searched.
    locations: Mutex<HashMap<String, (Instant, Option<PathBuf>)>>,
    /// Outcome of the most recent syntax checks, the latest used at the back.
    /// Texts may be large: only their hash is kept.
    checks: Mutex<VecDeque<(CheckKey, Option<String>)>>,
    /// Keyed at random for each process: a text cannot be crafted to share
    /// the hash of another one.
    hasher: RandomState,
}

impl Hurlfmt {
    /// Location of `hurlfmt` for this `hurlPath` preference (see
    /// [`locate_hurlfmt`]).
    pub fn locate(&self, hurl_path: &str) -> Option<PathBuf> {
        self.locate_with(hurl_path, Instant::now(), locate_hurlfmt)
    }

    fn locate_with(
        &self,
        hurl_path: &str,
        now: Instant,
        locate: impl FnOnce(&str) -> Option<PathBuf>,
    ) -> Option<PathBuf> {
        let mut locations = self.locations.lock().unwrap();
        // Also keeps the map small: outdated preferences do not pile up.
        locations
            .retain(|_, (searched, _)| now.saturating_duration_since(*searched) < LOCATION_TTL);
        if let Some((_, location)) = locations.get(hurl_path) {
            return location.clone();
        }
        let location = locate(hurl_path);
        locations.insert(hurl_path.to_string(), (now, location.clone()));
        location
    }

    /// Forgets everything. Called when the GUI asks where the binaries are
    /// (at startup and when the preference changes): they may have been
    /// installed, moved or upgraded.
    pub fn forget(&self) {
        self.locations.lock().unwrap().clear();
        self.checks.lock().unwrap().clear();
    }

    /// Syntax check with the installed `hurlfmt`. `None` when the text is
    /// valid or when `hurlfmt` is not available. The message is the one of
    /// `hurlfmt`, which reads the text from its standard input and names it
    /// `-`.
    pub fn syntax_error(&self, hurl_path: &str, text: &str) -> Option<String> {
        let hurlfmt = self.locate(hurl_path)?;
        match self.check_with(&hurlfmt, text, check_syntax) {
            Ok(error) => error,
            Err(_) => {
                // Moved, removed, not executable...: searched again the next
                // time.
                self.locations.lock().unwrap().remove(hurl_path);
                None
            }
        }
    }

    /// Outcome of `check` for this text, run only when the same text was not
    /// checked recently with the same `hurlfmt`. A check which could not be
    /// run is not remembered.
    fn check_with(
        &self,
        hurlfmt: &Path,
        text: &str,
        check: impl FnOnce(&Path, &str) -> Result<Option<String>, String>,
    ) -> Result<Option<String>, String> {
        let key: CheckKey = (self.hasher.hash_one((hurlfmt, text)), text.len());
        {
            let mut checks = self.checks.lock().unwrap();
            if let Some(known) = checks.iter().position(|(k, _)| *k == key) {
                let (_, error) = checks.remove(known).expect("position of an element");
                checks.push_back((key, error.clone()));
                return Ok(error);
            }
        }
        // Not locked while the process runs: other files can be checked.
        let error = check(hurlfmt, text)?;
        let mut checks = self.checks.lock().unwrap();
        checks.retain(|(k, _)| *k != key);
        if checks.len() >= MAX_CHECKS {
            checks.pop_front();
        }
        checks.push_back((key, error.clone()));
        Ok(error)
    }
}

/// Converts a `curl` command line into Hurl source with `hurlfmt --in curl`.
pub fn curl_to_hurl(hurlfmt: Option<&Path>, curl: &str) -> Result<String, String> {
    let hurlfmt =
        hurlfmt.ok_or("hurlfmt is required to import a curl command but was not found")?;
    let mut cmd = command(hurlfmt);
    cmd.args(["--no-color", "--in", "curl"]);
    // One command per line: line continuations are folded.
    let line = curl.replace("\\\r\n", " ").replace("\\\n", " ");
    let out = run_with_stdin(cmd, &format!("{}\n", line.trim()))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

// --- Running ----------------------------------------------------------------

/// Start of the name of the directory of a run (see `run_dir`), followed by
/// random characters.
const RUN_DIR_PREFIX: &str = "hurlui-run-";

/// Age from which a run directory is known to be abandoned. The directory of
/// a run in progress (in this process, in another instance of the application
/// or in the development bridge) must never be removed, and its modification
/// time may be as old as the start of that run. Hurl gives up on a request
/// after 300 seconds by default, but a file may hold many requests, retries
/// and a longer `--max-time`: a whole day is out of reach of any run which is
/// still looked at, and the price is only that what a killed application
/// left behind stays on the disk until a start of the application on a later
/// day.
const STALE_RUN_DIR_AGE: Duration = Duration::from_secs(24 * 60 * 60);

/// Private directory receiving the JSON report and the response bodies of one
/// run: unique, readable by the current user only, removed when dropped.
fn run_dir() -> Result<tempfile::TempDir, String> {
    let mut builder = tempfile::Builder::new();
    builder.prefix(RUN_DIR_PREFIX);
    #[cfg(unix)]
    {
        // The default would be 0777 minus the umask: response bodies are
        // written there, and the system temporary directory is shared.
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(fs::Permissions::from_mode(0o700));
    }
    builder
        .tempdir()
        .map_err(|e| format!("cannot create a temporary directory: {e}"))
}

/// Removes the run directories left in the system temporary directory by an
/// application which was killed during a run: nothing drops the directory
/// then (SIGKILL, or a panic which aborts), and it holds response bodies.
/// Best effort: every error is ignored, and nothing here panics.
pub(crate) fn sweep_stale_run_dirs() {
    sweep_run_dirs(&std::env::temp_dir(), STALE_RUN_DIR_AGE);
}

/// Removes from `dir` the run directories of the current user which were not
/// modified for `min_age`. Returns how many were removed.
fn sweep_run_dirs(dir: &Path, min_age: Duration) -> usize {
    // The system temporary directory is shared: only the directories of the
    // current user are ours to remove (and root could remove the others).
    // The user is the owner of what this process creates there; when that
    // cannot be told, nothing is removed.
    #[cfg(unix)]
    let owner = {
        use std::os::unix::fs::MetadataExt;
        match tempfile::tempfile_in(dir).and_then(|file| file.metadata()) {
            Ok(meta) => meta.uid(),
            Err(_) => return 0,
        }
    };
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    let now = SystemTime::now();
    let mut removed = 0;
    for entry in entries.flatten() {
        let ours = entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(RUN_DIR_PREFIX));
        if !ours {
            continue;
        }
        let path = entry.path();
        // Not followed: a symbolic link with such a name was not created by
        // `run_dir`, and what it points to must not be touched.
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if !meta.file_type().is_dir() {
            continue;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if meta.uid() != owner {
                continue;
            }
        }
        // A modification time in the future is not an age.
        let stale = meta
            .modified()
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age >= min_age);
        // Symbolic links inside the directory are removed, not followed.
        if stale && fs::remove_dir_all(&path).is_ok() {
            removed += 1;
        }
    }
    removed
}

fn quote(arg: &str) -> String {
    if !arg.is_empty()
        && arg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./=:@%+,".contains(c))
    {
        arg.to_string()
    } else {
        format!("'{}'", arg.replace('\'', "'\\''"))
    }
}

/// Command line of `hurl` for this request. The names of the options come
/// from the catalog of options shared with Hurl (see [`options`]).
fn arguments(req: &RunRequest, report_dir: &Path) -> Result<Vec<String>, String> {
    let mut args = vec![
        req.file.clone(),
        options::cli(options::REPORT_JSON)?,
        report_dir.to_string_lossy().into_owned(),
        options::cli(options::NO_OUTPUT)?,
        options::cli(options::NO_COLOR)?,
    ];
    match req.verbosity.as_str() {
        "verbose" => args.push(options::cli(options::VERBOSE)?),
        "very-verbose" => args.push(options::cli(options::VERY_VERBOSE)?),
        _ => {}
    }
    if let Some(n) = req.from_entry {
        args.extend([options::cli(options::FROM_ENTRY)?, n.to_string()]);
    }
    if let Some(n) = req.to_entry {
        args.extend([options::cli(options::TO_ENTRY)?, n.to_string()]);
    }
    if let Some(env) = req.environment.as_deref().filter(|e| !e.is_empty()) {
        let path = collection::environment_file(&req.collection, env);
        if path.is_file() {
            args.extend([
                options::cli(options::VARIABLES_FILE)?,
                path.to_string_lossy().into_owned(),
            ]);
        }
    }
    args.extend(options::arguments(&req.options)?);
    args.extend(req.extra_args.split_whitespace().map(str::to_string));
    Ok(args)
}

/// Whether this line was written by `bounded_output` where it dropped output:
/// its marker, followed by the end of the line which was cut (if any).
fn is_skipped_line(line: &str) -> bool {
    line.strip_prefix("[... ")
        .map(|rest| rest.trim_start_matches(|c: char| c.is_ascii_digit()))
        .is_some_and(|rest| rest.starts_with(" bytes of output skipped ...]"))
}

/// Hurl logs prefixed by `*`, `>`, `<` are the verbose trace; everything else
/// on stderr is an error report, but for what `bounded_output` left where it
/// dropped output (the end of a trace line has no prefix).
fn error_lines(stderr: &str) -> String {
    stderr
        .lines()
        .filter(|l| {
            !(l.starts_with("* ")
                || l.starts_with("> ")
                || l.starts_with("< ")
                || l.starts_with("** ")
                || matches!(*l, "*" | ">" | "<")
                || is_skipped_line(l))
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

fn header_value<'a>(headers: &'a Value, name: &str) -> Option<&'a str> {
    headers.as_array()?.iter().find_map(|h| {
        let n = h.get("name")?.as_str()?;
        n.eq_ignore_ascii_case(name)
            .then(|| h.get("value")?.as_str())
            .flatten()
    })
}

/// Reads at most `limit` bytes; the flag tells whether there were more.
fn read_limited(reader: impl Read, limit: usize) -> std::io::Result<(Vec<u8>, bool)> {
    let mut bytes = Vec::new();
    reader.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    let truncated = bytes.len() > limit;
    bytes.truncate(limit);
    Ok((bytes, truncated))
}

/// Content encodings which are decoded for display.
#[derive(Clone, Copy)]
enum Coding {
    Gzip,
    Deflate,
}

fn coding(encoding: Option<&str>) -> Option<Coding> {
    match encoding.map(|e| e.trim().to_ascii_lowercase()).as_deref() {
        Some("gzip") | Some("x-gzip") => Some(Coding::Gzip),
        Some("deflate") => Some(Coding::Deflate),
        _ => None,
    }
}

/// Reads a body stored by Hurl (as received) for display: decoded when the
/// content encoding is known, and never more than `limit` bytes in memory
/// whatever the size of the file or its compression ratio.
/// Returns `(bytes, truncated, decoded)`.
fn read_body(
    path: &Path,
    encoding: Option<&str>,
    limit: usize,
) -> std::io::Result<(Vec<u8>, bool, bool)> {
    let decoded = match coding(encoding) {
        Some(Coding::Gzip) => {
            read_limited(flate2::read::GzDecoder::new(File::open(path)?), limit).ok()
        }
        Some(Coding::Deflate) => {
            read_limited(flate2::read::ZlibDecoder::new(File::open(path)?), limit).ok()
        }
        None => None,
    };
    match decoded {
        Some((bytes, truncated)) => Ok((bytes, truncated, true)),
        None => {
            // Unknown encoding, or a body which is not what its header says.
            let (bytes, truncated) = read_limited(File::open(path)?, limit)?;
            Ok((bytes, truncated, false))
        }
    }
}

/// What is loaded of the response bodies of one run.
struct BodyBudget {
    /// Most bytes loaded from one body.
    per_body: usize,
    /// Bytes of bodies the result of the run may hold.
    per_run: usize,
    /// Bytes held so far: text, or image before its base64 encoding.
    loaded: usize,
}

impl BodyBudget {
    fn new(per_body: usize, per_run: usize) -> Self {
        BodyBudget {
            per_body,
            per_run,
            loaded: 0,
        }
    }

    /// Whether a body stored in `size` bytes is loaded. Never in part: a
    /// body cut for a reason other than its own size would be misleading.
    fn allows(&self, size: u64, coding: Option<Coding>) -> bool {
        // An empty body costs nothing. And as long as nothing is held, a
        // body is loaded as if it were the only one of the run.
        if size == 0 || self.loaded == 0 {
            return true;
        }
        let remaining = self.per_run.saturating_sub(self.loaded);
        match coding {
            // What it inflates to is only known once read: loaded as long as
            // the budget is not spent, which may exceed it by one body.
            Some(_) => remaining > 0,
            None => size.min(self.per_body as u64) <= remaining as u64,
        }
    }
}

/// Replaces the path of a stored response body by its content, or by its
/// description only (`omitted`) when the budget of the run does not allow it.
fn load_body(report_dir: &Path, response: &mut Value, budget: &mut BodyBudget) {
    let Some(rel) = response
        .get("body")
        .and_then(Value::as_str)
        .map(str::to_string)
    else {
        return;
    };
    // The report is only trusted to name files inside its own directory.
    let inside = Path::new(&rel)
        .components()
        .all(|c| matches!(c, Component::Normal(_)));
    let path = report_dir.join(&rel);
    let encoding = header_value(&response["headers"], "Content-Encoding").map(str::to_string);
    let coding = coding(encoding.as_deref());
    let image = header_value(&response["headers"], "Content-Type")
        .is_some_and(|t| t.trim().to_ascii_lowercase().starts_with("image/"));
    let stored = if inside {
        fs::metadata(&path).ok().filter(|meta| meta.is_file())
    } else {
        None
    };
    let Some(size) = stored.map(|meta| meta.len()) else {
        response["body"] = Value::Null;
        return;
    };
    if !budget.allows(size, coding) {
        // Not read at all. `encoded` is what it would most likely be: only
        // reading tells that a body is not what its header says.
        response["body"] = json!({
            "size": size,
            "truncated": false,
            "encoded": encoding.is_some() && coding.is_none(),
            "omitted": true,
        });
        return;
    }
    let Ok((bytes, truncated, decoded)) = read_body(&path, encoding.as_deref(), budget.per_body)
    else {
        response["body"] = Value::Null;
        return;
    };
    let mut body = json!({
        "size": size,
        "truncated": truncated,
        "encoded": encoding.is_some() && !decoded,
    });
    match String::from_utf8(bytes) {
        Ok(text) => {
            budget.loaded += text.len();
            body["text"] = Value::String(text);
        }
        Err(e) => {
            let valid = e.utf8_error().valid_up_to();
            let bytes = e.into_bytes();
            if truncated && bytes.len() - valid < 4 {
                // Text whose last character was cut by the truncation.
                budget.loaded += valid;
                body["text"] = Value::String(String::from_utf8_lossy(&bytes[..valid]).into_owned());
            } else if image && !truncated && bytes.len() <= MAX_IMAGE_SIZE {
                budget.loaded += bytes.len();
                body["base64"] =
                    Value::String(base64::engine::general_purpose::STANDARD.encode(bytes));
            }
            // Any other binary body is described by its size only, and
            // costs nothing.
        }
    }
    response["body"] = body;
}

/// Splits the error output into blocks (`error: ...` up to the next one) with
/// the source line of their `--> file:line:column` location, when present.
fn error_blocks(errors: &str) -> Vec<(Option<usize>, String)> {
    let mut blocks: Vec<(bool, Vec<&str>)> = vec![];
    for line in errors.lines() {
        if line.starts_with("error:") || line.starts_with("warning:") {
            blocks.push((line.starts_with("error:"), vec![line]));
        } else if let Some((_, lines)) = blocks.last_mut() {
            lines.push(line);
        }
    }
    blocks
        .into_iter()
        .filter(|(is_error, _)| *is_error)
        .map(|(_, lines)| {
            let location = lines.iter().find_map(|l| {
                let mut parts = l.trim().strip_prefix("--> ")?.rsplitn(3, ':');
                let _column = parts.next()?;
                parts.next()?.parse::<usize>().ok()
            });
            (location, lines.join("\n").trim_end().to_string())
        })
        .collect()
}

fn normalized(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The JSON report only knows about asserts: runtime errors (undefined
/// variable, connection failure, invalid capture...) are attached to their
/// entry from the error output, and every entry gets a `failed` flag.
fn attach_errors(entries: &mut [Value], errors: &str) {
    let line_of = |e: &Value| e.get("line").and_then(Value::as_u64).unwrap_or(0) as usize;
    for (location, text) in error_blocks(errors) {
        let Some(location) = location else { continue };
        // Entry holding the line; the last attempt when the entry was retried.
        let mut target: Option<usize> = None;
        for (i, e) in entries.iter().enumerate() {
            let line = line_of(e);
            if line <= location && target.is_none_or(|t| line >= line_of(&entries[t])) {
                target = Some(i);
            }
        }
        let Some(entry) = target.map(|t| &mut entries[t]) else {
            continue;
        };
        let message = normalized(text.strip_prefix("error:").unwrap_or(&text));
        let is_assert = entry["asserts"].as_array().is_some_and(|asserts| {
            asserts
                .iter()
                .filter_map(|a| a.get("message").and_then(Value::as_str))
                .any(|m| normalized(m) == message)
        });
        if !is_assert {
            match entry.get_mut("errors").and_then(Value::as_array_mut) {
                Some(list) => list.push(Value::String(text)),
                None => entry["errors"] = json!([text]),
            }
        }
    }
    for entry in entries {
        let failed_assert = entry["asserts"]
            .as_array()
            .is_some_and(|a| a.iter().any(|x| x["success"] == false));
        let has_errors = entry.get("errors").is_some();
        if !has_errors {
            entry["errors"] = json!([]);
        }
        entry["failed"] = Value::Bool(failed_assert || has_errors);
    }
}

/// Reads `report.json` and returns `(success, entries)`.
fn read_report(report_dir: &Path, source: &str, errors: &str) -> Option<(bool, Value)> {
    let budget = BodyBudget::new(MAX_BODY_SIZE, MAX_RUN_BODIES);
    read_report_within(report_dir, source, errors, budget)
}

/// `read_report` with the given limits on the bodies which are loaded.
fn read_report_within(
    report_dir: &Path,
    source: &str,
    errors: &str,
    mut budget: BodyBudget,
) -> Option<(bool, Value)> {
    let text = fs::read_to_string(report_dir.join("report.json")).ok()?;
    let report: Value = serde_json::from_str(&text).ok()?;
    let mut file = report.as_array()?.first()?.clone();
    let success = file
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let lines: Vec<&str> = source.lines().collect();
    if let Some(entries) = file.get_mut("entries").and_then(Value::as_array_mut) {
        // Backwards, so that the budget goes first to the bodies which are
        // the most likely to be looked at: a request sent with the ones it
        // depends on is the last entry of the run, and the call shown for an
        // entry is its last one (the others led to it: redirections). The
        // order of the entries and of the calls themselves does not change.
        for entry in entries.iter_mut().rev() {
            if let Some(calls) = entry.get_mut("calls").and_then(Value::as_array_mut) {
                for call in calls.iter_mut().rev() {
                    if let Some(response) = call.get_mut("response") {
                        load_body(report_dir, response, &mut budget);
                    }
                }
            }
            if let Some(asserts) = entry.get_mut("asserts").and_then(Value::as_array_mut) {
                for assert in asserts {
                    let line = assert.get("line").and_then(Value::as_u64).unwrap_or(0) as usize;
                    if let Some(text) = line.checked_sub(1).and_then(|i| lines.get(i)) {
                        assert["source"] = Value::String(text.trim().to_string());
                    }
                }
            }
        }
    }
    if let Some(entries) = file.get_mut("entries").and_then(Value::as_array_mut) {
        attach_errors(entries, errors);
    }
    Some((success, file["entries"].take()))
}

/// Reads `reader` to its end (the child must never block on a full pipe) but
/// keeps only its first and last `half` bytes: a very verbose run logs every
/// body. Errors are reported at the end of the output, which is preserved.
fn bounded_output(reader: &mut impl Read, half: usize) -> String {
    let mut head: Vec<u8> = Vec::new();
    let mut tail: Vec<u8> = Vec::new();
    let mut skipped = 0usize;
    // Whether `tail` starts with a whole line (not with the end of a line
    // whose beginning was dropped).
    let mut whole_line = true;
    let mut chunk = [0u8; 16 * 1024];
    loop {
        let n = match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(_) => break,
        };
        let mut data = &chunk[..n];
        if head.len() < half {
            let take = (half - head.len()).min(data.len());
            head.extend_from_slice(&data[..take]);
            data = &data[take..];
        }
        tail.extend_from_slice(data);
        if tail.len() > 2 * half {
            let cut = tail.len() - half;
            skipped += cut;
            whole_line = tail[cut - 1] == b'\n';
            tail.drain(..cut);
        }
    }
    if tail.len() > half {
        let cut = tail.len() - half;
        skipped += cut;
        whole_line = tail[cut - 1] == b'\n';
        tail.drain(..cut);
    }
    let mut out = String::from_utf8_lossy(&head).into_owned();
    if skipped > 0 {
        // The end of a line which was cut stays on the line of the marker:
        // neither was written as such by the process (see `error_lines`).
        out.push_str(&format!("\n[... {skipped} bytes of output skipped ...]"));
        out.push(if whole_line { '\n' } else { ' ' });
    }
    out.push_str(&String::from_utf8_lossy(&tail));
    out
}

fn read_pipe<R: Read + Send + 'static>(pipe: Option<R>) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || match pipe {
        Some(mut pipe) => bounded_output(&mut pipe, OUTPUT_HALF),
        None => String::new(),
    })
}

pub fn run(runs: &Runs, secrets: &[KeyValue], req: &RunRequest) -> Result<Value, String> {
    let hurl = locate("hurl", &req.hurl_path).ok_or_else(|| {
        if req.hurl_path.trim().is_empty() {
            "hurl binary not found: install Hurl (https://hurl.dev) or set its path in the preferences".to_string()
        } else {
            format!("hurl binary not found at {}", req.hurl_path)
        }
    })?;
    let source = fs::read_to_string(&req.file).map_err(|e| format!("{}: {e}", req.file))?;
    let report_dir = run_dir()?;
    let report_dir = report_dir.path();

    let args = arguments(req, report_dir)?;
    let mut cmd = command(&hurl);
    cmd.args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if Path::new(&req.collection).is_dir() {
        cmd.current_dir(&req.collection);
    }
    // Secrets go through the environment: they do not show up in the process
    // list and Hurl redacts them from its logs and reports.
    for s in secrets {
        cmd.env(format!("HURL_SECRET_{}", s.name), &s.value);
    }
    let shown = std::iter::once(hurl.to_string_lossy().into_owned())
        .chain(args.iter().cloned())
        .map(|a| quote(&a))
        .collect::<Vec<_>>()
        .join(" ");

    let cancel = runs.register(&req.run_id);
    let started = Instant::now();
    let spawned = cmd.spawn();
    let mut child = match spawned {
        Ok(c) => c,
        Err(e) => {
            runs.unregister(&req.run_id);
            return Err(format!("cannot start {}: {e}", hurl.display()));
        }
    };
    let stdout = read_pipe(child.stdout.take());
    let stderr = read_pipe(child.stderr.take());
    let mut cancelled = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if cancel.load(Ordering::SeqCst) && !cancelled {
                    cancelled = true;
                    let _ = child.kill();
                }
                std::thread::sleep(Duration::from_millis(15));
            }
            Err(_) => break None,
        }
    };
    runs.unregister(&req.run_id);
    let _ = stdout.join();
    let stderr = stderr.join().unwrap_or_default();
    let duration = started.elapsed().as_millis() as u64;

    let errors = error_lines(&stderr);
    let report = read_report(report_dir, &source, &errors);
    let exit_code = status.and_then(|s| s.code());
    let (success, entries) = match report {
        Some((success, entries)) => (success && !cancelled, entries),
        None => (exit_code == Some(0) && !cancelled, json!([])),
    };
    // Every value is moved into the result: `json!` serializes what it is
    // given by reference, which would copy each loaded body (and the output)
    // while the original is still in memory.
    let mut result = serde_json::Map::new();
    result.insert("runId".into(), Value::String(req.run_id.clone()));
    result.insert("file".into(), Value::String(req.file.clone()));
    result.insert("command".into(), Value::String(shown));
    result.insert(
        "exitCode".into(),
        exit_code.map_or(Value::Null, Value::from),
    );
    result.insert("cancelled".into(), Value::Bool(cancelled));
    result.insert("durationMs".into(), Value::from(duration));
    result.insert("success".into(), Value::Bool(success));
    result.insert("entries".into(), entries);
    result.insert("errors".into(), Value::String(errors));
    result.insert("stderr".into(), Value::String(stderr));
    Ok(Value::Object(result))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_errors_from_verbose_trace() {
        let stderr = "* Executing entry 1\n> GET /x HTTP/1.1\n>\n< HTTP/1.1 404\n<\n*\nerror: Assert status code\n  --> a.hurl:2:6\n   |\n 2 | HTTP 200\n   |      ^^^ actual value is <404>\n   |\n";
        let errors = error_lines(stderr);
        assert!(errors.starts_with("error: Assert status code"));
        assert!(errors.ends_with("   |"));
        assert!(!errors.contains("Executing"));
    }

    #[test]
    fn attaches_runtime_errors_to_entries() {
        let errors = "error: Undefined variable\n  --> /c/a.hurl:6:23\n   |\n 6 | Authorization: {{token}}\n   |\n\nerror: Assert status code\n  --> /c/a.hurl:10:6\n   |\n10 | HTTP 200\n   |      ^^^ actual value is <404>\n   |\n\nwarning: something\n  --> /c/a.hurl:1:1\nerror: no location";
        let mut entries = vec![
            json!({ "index": 1, "line": 1, "asserts": [{ "success": true, "line": 2 }] }),
            json!({ "index": 2, "line": 5, "asserts": [] }),
            json!({ "index": 3, "line": 9, "asserts": [{
                "success": false, "line": 10,
                "message": "Assert status code\n  --> /c/a.hurl:10:6\n   |\n10 | HTTP 200\n   |      ^^^ actual value is <404>\n   |"
            }] }),
        ];
        attach_errors(&mut entries, errors);
        assert_eq!(entries[0]["failed"], false);
        assert_eq!(entries[0]["errors"], json!([]));
        assert_eq!(entries[1]["failed"], true);
        assert!(
            entries[1]["errors"][0]
                .as_str()
                .unwrap()
                .starts_with("error: Undefined variable")
        );
        // The assert failure is already described by the assert itself.
        assert_eq!(entries[2]["failed"], true);
        assert_eq!(entries[2]["errors"], json!([]));
    }

    #[test]
    fn arguments_follow_the_request() {
        let req = RunRequest {
            file: "a b.hurl".into(),
            from_entry: Some(2),
            to_entry: Some(2),
            options: json!({ "insecure": true, "max-time": 30, "location": false })
                .as_object()
                .unwrap()
                .clone(),
            verbosity: "verbose".into(),
            extra_args: "--retry 2".into(),
            ..RunRequest::default()
        };
        let args = arguments(&req, Path::new("/tmp/r")).unwrap();
        assert_eq!(
            args,
            [
                "a b.hurl",
                "--report-json",
                "/tmp/r",
                "--no-output",
                "--no-color",
                "--verbose",
                "--from-entry",
                "2",
                "--to-entry",
                "2",
                "--insecure",
                "--max-time",
                "30",
                "--retry",
                "2"
            ]
        );
        let unknown = RunRequest {
            options: json!({ "nope": true }).as_object().unwrap().clone(),
            ..RunRequest::default()
        };
        assert_eq!(
            arguments(&unknown, Path::new("/tmp/r")).unwrap_err(),
            "unknown hurl option: --nope"
        );
        assert_eq!(quote("a b.hurl"), "'a b.hurl'");
        assert_eq!(quote("--max-time"), "--max-time");
    }

    fn gzip(data: &[u8]) -> Vec<u8> {
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(data).unwrap();
        enc.finish().unwrap()
    }

    #[test]
    fn run_directories_are_private_and_removed() {
        let (first, second) = (run_dir().unwrap(), run_dir().unwrap());
        assert_ne!(first.path(), second.path());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(first.path()).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o700, "{mode:o}");
        }
        let path = first.path().to_path_buf();
        fs::write(path.join("report.json"), "[]").unwrap();
        drop(first);
        assert!(!path.exists());
    }

    #[test]
    fn bodies_are_decoded_within_a_memory_limit() {
        let dir = run_dir().unwrap();
        let path = dir.path().join("body");

        fs::write(&path, gzip(b"hello")).unwrap();
        let (bytes, truncated, decoded) = read_body(&path, Some("GZip"), 100).unwrap();
        assert_eq!(
            (bytes.as_slice(), truncated, decoded),
            (&b"hello"[..], false, true)
        );

        // A small file inflating to 8 MB: only `limit` bytes are ever kept.
        let bomb = gzip(&vec![0u8; 8 * 1024 * 1024]);
        assert!(bomb.len() < 64 * 1024);
        fs::write(&path, bomb).unwrap();
        let (bytes, truncated, decoded) = read_body(&path, Some("gzip"), 1024).unwrap();
        assert_eq!((bytes.len(), truncated, decoded), (1024, true, true));

        // Not what the header says, or an encoding we cannot decode: raw bytes.
        fs::write(&path, b"plain text").unwrap();
        let (bytes, truncated, decoded) = read_body(&path, Some("gzip"), 5).unwrap();
        assert_eq!(
            (bytes.as_slice(), truncated, decoded),
            (&b"plain"[..], true, false)
        );
        let (bytes, _, decoded) = read_body(&path, Some("br"), 100).unwrap();
        assert_eq!((bytes.as_slice(), decoded), (&b"plain text"[..], false));
    }

    #[test]
    fn loads_bodies_from_the_report_directory_only() {
        let dir = run_dir().unwrap();
        fs::create_dir(dir.path().join("store")).unwrap();
        fs::write(dir.path().join("store/a.json"), b"{\"a\":1}").unwrap();
        fs::write(dir.path().join("store/b.bin"), [0u8, 159, 146, 150]).unwrap();
        let load = |body: &str, content_type: &str| {
            let mut response = json!({
                "body": body,
                "headers": [{ "name": "content-type", "value": content_type }],
            });
            load_body(
                dir.path(),
                &mut response,
                &mut BodyBudget::new(MAX_BODY_SIZE, MAX_RUN_BODIES),
            );
            response["body"].take()
        };
        let text = load("store/a.json", "application/json");
        assert_eq!(
            (&text["text"], &text["size"]),
            (&json!("{\"a\":1}"), &json!(7))
        );
        // Binary: previewable images are sent, anything else is size only.
        assert!(load("store/b.bin", "image/png")["base64"].is_string());
        let binary = load("store/b.bin", "application/octet-stream");
        assert_eq!(binary["size"], 4);
        assert!(binary.get("base64").is_none() && binary.get("text").is_none());
        // Paths escaping the report directory are ignored.
        let outside = dir
            .path()
            .join("store/a.json")
            .to_string_lossy()
            .into_owned();
        assert_eq!(load(&outside, "application/json"), Value::Null);
        assert_eq!(
            load("store/../store/a.json", "application/json"),
            Value::Null
        );
    }

    /// A call of a report, its response body stored in `dir` as Hurl does.
    fn call(dir: &Path, name: &str, content: &[u8], headers: &[(&str, &str)]) -> Value {
        fs::create_dir_all(dir.join("store")).unwrap();
        fs::write(dir.join("store").join(name), content).unwrap();
        let headers: Vec<Value> = headers
            .iter()
            .map(|(name, value)| json!({ "name": name, "value": value }))
            .collect();
        json!({ "response": { "status": 200, "headers": headers, "body": format!("store/{name}") } })
    }

    /// Writes the report of a file whose entries made these calls, and reads
    /// it with these limits on the bodies. Returns the entries.
    fn read_entries(
        dir: &Path,
        entries: Vec<Vec<Value>>,
        per_body: usize,
        per_run: usize,
    ) -> Value {
        let entries: Vec<Value> = entries
            .into_iter()
            .enumerate()
            .map(|(i, calls)| json!({ "index": i + 1, "line": i + 1, "calls": calls, "asserts": [] }))
            .collect();
        let report = json!([{ "success": true, "entries": entries }]);
        fs::write(dir.join("report.json"), report.to_string()).unwrap();
        let (success, entries) =
            read_report_within(dir, "", "", BodyBudget::new(per_body, per_run)).unwrap();
        assert!(success);
        entries
    }

    /// Body of a call, both numbered from 1.
    fn body(entries: &Value, entry: usize, call: usize) -> &Value {
        &entries[entry - 1]["calls"][call - 1]["response"]["body"]
    }

    fn text_body(size: u64, text: &str) -> Value {
        json!({ "size": size, "truncated": false, "encoded": false, "text": text })
    }

    fn omitted_body(size: u64) -> Value {
        json!({ "size": size, "truncated": false, "encoded": false, "omitted": true })
    }

    #[test]
    fn bodies_beyond_the_budget_of_the_run_are_omitted() {
        let dir = run_dir().unwrap();
        let dir = dir.path();
        let plain = [("Content-Type", "text/plain")];
        let entries = vec![
            vec![call(dir, "tiny", b"tiny", &plain)],
            vec![call(dir, "first", b"aaaaaaaaaa", &plain)],
            // A redirection, then the response which is displayed.
            vec![
                call(dir, "moved", b"bbbbbbbbbb", &plain),
                call(dir, "target", b"cccccccccc", &plain),
            ],
            vec![call(dir, "last", b"dddddddddd", &plain)],
        ];

        // Served from the last entry, and from the last call of an entry.
        let read = read_entries(dir, entries.clone(), 100, 25);
        assert_eq!(body(&read, 4, 1), &text_body(10, "dddddddddd"));
        assert_eq!(body(&read, 3, 2), &text_body(10, "cccccccccc"));
        // Never in part: 5 bytes are left.
        assert_eq!(body(&read, 3, 1), &omitted_body(10));
        assert_eq!(body(&read, 2, 1), &omitted_body(10));
        // A body which fits in what is left is still loaded.
        assert_eq!(body(&read, 1, 1), &text_body(4, "tiny"));
        // Entries and calls are where they were.
        let entries_read = read.as_array().unwrap();
        assert_eq!(entries_read.len(), 4);
        for (i, entry) in entries_read.iter().enumerate() {
            assert_eq!(entry["index"], i + 1);
            assert_eq!(entry["failed"], false);
        }
        assert_eq!(entries_read[2]["calls"].as_array().unwrap().len(), 2);

        // Once the budget is spent, nothing else is loaded.
        let read = read_entries(dir, entries.clone(), 100, 20);
        assert_eq!(body(&read, 4, 1), &text_body(10, "dddddddddd"));
        assert_eq!(body(&read, 3, 2), &text_body(10, "cccccccccc"));
        assert_eq!(body(&read, 3, 1), &omitted_body(10));
        assert_eq!(body(&read, 2, 1), &omitted_body(10));
        assert_eq!(body(&read, 1, 1), &omitted_body(4));

        // A budget which is large enough changes nothing.
        let read = read_entries(dir, entries, 100, 44);
        for (entry, call, text) in [(1, 1, "tiny"), (2, 1, "aaaaaaaaaa"), (3, 1, "bbbbbbbbbb")] {
            assert_eq!(
                body(&read, entry, call),
                &text_body(text.len() as u64, text)
            );
        }
    }

    #[test]
    fn the_first_body_is_loaded_whatever_the_budget() {
        let dir = run_dir().unwrap();
        let dir = dir.path();
        let plain = [("Content-Type", "text/plain")];
        let large = "0123456789".repeat(3);
        let truncated =
            json!({ "size": 30, "truncated": true, "encoded": false, "text": "0123456789" });

        // A single body larger than every limit: as without a budget.
        let read = read_entries(
            dir,
            vec![vec![call(dir, "only", large.as_bytes(), &plain)]],
            10,
            4,
        );
        assert_eq!(body(&read, 1, 1), &truncated);

        let entries = vec![
            vec![call(dir, "empty", b"", &plain)],
            vec![call(dir, "small", b"x", &plain)],
            vec![call(dir, "large", large.as_bytes(), &plain)],
            vec![call(dir, "none", b"", &plain)],
            vec![call(dir, "last", large.as_bytes(), &plain)],
        ];
        // What a body costs is what is kept of it. An omitted body tells the
        // size of what is stored.
        let read = read_entries(dir, entries.clone(), 10, 15);
        assert_eq!(body(&read, 5, 1), &truncated);
        assert_eq!(body(&read, 3, 1), &omitted_body(30));
        assert_eq!(body(&read, 2, 1), &text_body(1, "x"));
        // Empty bodies cost nothing and are never omitted, even when they
        // come first or once the budget is spent.
        let read = read_entries(dir, entries, 10, 10);
        assert_eq!(body(&read, 5, 1), &truncated);
        assert_eq!(body(&read, 4, 1), &text_body(0, ""));
        assert_eq!(body(&read, 3, 1), &omitted_body(30));
        assert_eq!(body(&read, 2, 1), &omitted_body(1));
        assert_eq!(body(&read, 1, 1), &text_body(0, ""));
    }

    #[test]
    fn encoded_bodies_are_charged_for_what_they_inflate_to() {
        let dir = run_dir().unwrap();
        let dir = dir.path();
        let zipped = [("Content-Type", "text/plain"), ("Content-Encoding", "gzip")];
        let (a, b, c) = (gzip(&[b'a'; 30]), gzip(&[b'b'; 30]), gzip(&[b'c'; 30]));
        assert!(a.len() < 30);
        let entries = vec![
            vec![call(dir, "a", &a, &zipped)],
            vec![call(
                dir,
                "br",
                b"\x8b\x03\x80data\x03",
                &[("Content-Encoding", "br")],
            )],
            vec![call(dir, "b", &b, &zipped)],
            vec![call(dir, "c", &c, &zipped)],
        ];
        let read = read_entries(dir, entries, 100, 40);
        assert_eq!(
            body(&read, 4, 1),
            &text_body(c.len() as u64, &"c".repeat(30))
        );
        // Its size is not known before it is read: loaded while the budget
        // is not spent, even if it does not fit.
        assert_eq!(
            body(&read, 3, 1),
            &text_body(b.len() as u64, &"b".repeat(30))
        );
        // Not read: the size is the one of the stored file, and a body which
        // would not have been decoded is told as such.
        assert_eq!(
            body(&read, 2, 1),
            &json!({ "size": 8, "truncated": false, "encoded": true, "omitted": true })
        );
        assert_eq!(body(&read, 1, 1), &omitted_body(a.len() as u64));
    }

    #[test]
    fn only_what_is_kept_of_a_body_is_charged() {
        let dir = run_dir().unwrap();
        let dir = dir.path();
        let entries = vec![
            vec![call(
                dir,
                "text",
                b"0123456789",
                &[("Content-Type", "text/plain")],
            )],
            vec![call(
                dir,
                "image",
                &[0u8, 159, 146, 150],
                &[("Content-Type", "image/png")],
            )],
            vec![call(
                dir,
                "binary",
                &[159u8; 50],
                &[("Content-Type", "application/octet-stream")],
            )],
        ];
        // A binary body is described by its size only: it costs nothing. An
        // image costs its bytes, not the length of their base64 encoding.
        let read = read_entries(dir, entries.clone(), 100, 14);
        assert_eq!(
            body(&read, 3, 1),
            &json!({ "size": 50, "truncated": false, "encoded": false })
        );
        assert_eq!(
            body(&read, 2, 1),
            &json!({ "size": 4, "truncated": false, "encoded": false, "base64": "AJ+Slg==" })
        );
        assert_eq!(body(&read, 1, 1), &text_body(10, "0123456789"));
        let read = read_entries(dir, entries, 100, 13);
        assert!(body(&read, 2, 1)["base64"].is_string());
        assert_eq!(body(&read, 1, 1), &omitted_body(10));
    }

    /// Makes `path` (a file or a directory) as old as `age`.
    fn backdate(path: &Path, age: Duration) {
        let mut options = fs::OpenOptions::new();
        #[cfg(not(windows))]
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            // FILE_FLAG_BACKUP_SEMANTICS: required to open a directory.
            options.write(true).custom_flags(0x0200_0000);
        }
        let file = options.open(path).unwrap();
        file.set_modified(SystemTime::now() - age).unwrap();
    }

    #[test]
    fn sweeps_stale_run_directories_only() {
        let day = Duration::from_secs(24 * 60 * 60);
        let root = tempfile::tempdir().unwrap();
        let tmp = root.path().join("tmp");
        let elsewhere = root.path().join("elsewhere");
        fs::create_dir_all(&tmp).unwrap();
        fs::create_dir_all(elsewhere.join("kept")).unwrap();
        fs::write(elsewhere.join("kept/file"), "kept").unwrap();
        let name = |suffix: &str| tmp.join(format!("{RUN_DIR_PREFIX}{suffix}"));

        // Left by a killed application two days ago, as Hurl filled it.
        let stale = name("stale1");
        fs::create_dir_all(stale.join("store")).unwrap();
        fs::write(stale.join("report.json"), "[]").unwrap();
        fs::write(stale.join("store/body_response"), "secret").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&elsewhere, stale.join("link")).unwrap();
        // A run in progress somewhere else.
        let recent = name("recent");
        fs::create_dir(&recent).unwrap();
        fs::write(recent.join("report.json"), "[]").unwrap();
        // Not what `run_dir` creates, whatever their age.
        let file = name("file");
        fs::write(&file, "file").unwrap();
        let other = tmp.join("other-run-stale1");
        fs::create_dir(&other).unwrap();
        #[cfg(unix)]
        let link = name("link");
        #[cfg(unix)]
        std::os::unix::fs::symlink(elsewhere.join("kept"), &link).unwrap();
        for old in [&stale, &file, &other, &elsewhere.join("kept")] {
            backdate(old, 2 * day);
        }

        assert_eq!(sweep_run_dirs(&tmp, day), 1);
        assert!(!stale.exists());
        assert!(recent.join("report.json").is_file());
        assert!(file.is_file() && other.is_dir());
        // Nothing was followed out of the temporary directory.
        assert_eq!(
            fs::read_to_string(elsewhere.join("kept/file")).unwrap(),
            "kept"
        );
        #[cfg(unix)]
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(sweep_run_dirs(&tmp, day), 0);

        // Only its age protects a run directory; the rest is never ours.
        assert_eq!(sweep_run_dirs(&tmp, Duration::ZERO), 1);
        assert!(!recent.exists());
        assert!(file.is_file() && other.is_dir());
        assert_eq!(
            fs::read_to_string(elsewhere.join("kept/file")).unwrap(),
            "kept"
        );
        // A directory which is missing or cannot be read is not an error.
        assert_eq!(sweep_run_dirs(&tmp.join("missing"), Duration::ZERO), 0);
    }

    #[cfg(unix)]
    #[test]
    fn sweep_leaves_the_run_directories_of_other_users() {
        let day = Duration::from_secs(24 * 60 * 60);
        let tmp = tempfile::tempdir().unwrap();
        let foreign = tmp.path().join(format!("{RUN_DIR_PREFIX}foreign"));
        fs::create_dir(&foreign).unwrap();
        backdate(&foreign, 2 * day);
        // Only root can give a directory away (and could remove the ones of
        // the others, hence the check).
        if std::os::unix::fs::chown(&foreign, Some(12345), None).is_err() {
            eprintln!("not root: test skipped");
            return;
        }
        assert_eq!(sweep_run_dirs(tmp.path(), day), 0);
        assert!(foreign.is_dir());
    }

    #[test]
    fn output_is_bounded_and_keeps_both_ends() {
        let short = bounded_output(&mut &b"error: boom\n"[..], 8);
        assert_eq!(short, "error: boom\n");
        let long = format!("START{}END", "x".repeat(100_000));
        let out = bounded_output(&mut long.as_bytes(), 16);
        assert!(out.starts_with("START") && out.ends_with("END"));
        assert!(out.contains("bytes of output skipped"));
        assert!(out.len() < 100);
    }

    #[test]
    fn skipped_output_is_not_an_error() {
        let error = "error: Assert status code\n  --> a.hurl:2:6\n   |\n 2 | HTTP 200\n   |      ^^^ actual value is <404>\n   |";
        // Very verbose run: every line of the body is logged as a trace line.
        let body = "* 0123456789 abcdefghijklmnopqrstuvwxyz\n".repeat(500);
        let failed = format!("* Executing entry 1\n{body}*\n{error}\n\n* Writing JSON report\n");
        let passed = format!("* Executing entry 1\n{body}*\n* Writing JSON report\n");
        // Wherever the output is cut: in a line, or between two lines.
        for half in 150..250 {
            let out = bounded_output(&mut failed.as_bytes(), half);
            assert!(out.contains("bytes of output skipped"));
            assert_eq!(error_lines(&out), error, "half: {half}");
            let out = bounded_output(&mut passed.as_bytes(), half);
            assert!(out.contains("bytes of output skipped"));
            assert_eq!(error_lines(&out), "", "half: {half}");
        }
        // A body on a single line: what is kept of it is not an error either.
        let one_line = format!("* {}\n*\n{error}\n", "x".repeat(10_000));
        let out = bounded_output(&mut one_line.as_bytes(), 400);
        assert!(out.starts_with("* xxx") && out.contains("bytes of output skipped"));
        assert_eq!(error_lines(&out), error);
        // A line which is kept whole is never dropped, even the first one
        // after the cut.
        let last = "error: boom\n  --> a.hurl:1:1\n";
        let out = bounded_output(
            &mut format!("{}{last}", "* trace\n".repeat(100)).as_bytes(),
            last.len(),
        );
        assert_eq!(error_lines(&out), last.trim_end());
    }

    #[test]
    fn hurlfmt_location_is_remembered_for_a_short_time() {
        use std::cell::Cell;
        let cache = Hurlfmt::default();
        let searches = Cell::new(0);
        let found = |hurl_path: &str| {
            searches.set(searches.get() + 1);
            (!hurl_path.is_empty()).then(|| PathBuf::from(format!("{hurl_path}fmt")))
        };
        let start = Instant::now();
        let at = |seconds: u64| start + Duration::from_secs(seconds);

        assert_eq!(
            cache.locate_with("/bin/hurl", at(0), found),
            Some(PathBuf::from("/bin/hurlfmt"))
        );
        assert_eq!(
            cache.locate_with("/bin/hurl", at(29), found),
            Some(PathBuf::from("/bin/hurlfmt"))
        );
        assert_eq!(searches.get(), 1);
        // One location for each preference; "not found" is remembered too.
        assert_eq!(cache.locate_with("", at(20), found), None);
        assert_eq!(cache.locate_with("", at(40), found), None);
        assert_eq!(searches.get(), 2);
        // Searched again once outdated: Hurl may have been installed since.
        assert_eq!(
            cache.locate_with("/bin/hurl", at(31), found),
            Some(PathBuf::from("/bin/hurlfmt"))
        );
        assert_eq!(cache.locate_with("", at(50), found), None);
        assert_eq!(searches.get(), 4);
        assert!(cache.locations.lock().unwrap().len() <= 2);
        // And when the GUI asks for the binaries again.
        cache.forget();
        cache.locate_with("/bin/hurl", at(51), found);
        assert_eq!(searches.get(), 5);
    }

    #[test]
    fn syntax_checks_are_remembered_by_content() {
        use std::cell::Cell;
        let cache = Hurlfmt::default();
        let runs = Cell::new(0);
        // Stands for hurlfmt: texts holding `!` are invalid.
        let check = |hurlfmt: &Path, text: &str| {
            runs.set(runs.get() + 1);
            Ok(text
                .contains('!')
                .then(|| format!("error by {}\n  --> -:1:1", hurlfmt.display())))
        };
        let (fmt, other) = (Path::new("/bin/hurlfmt"), Path::new("/opt/hurlfmt"));

        assert_eq!(cache.check_with(fmt, "GET http://a\n", check), Ok(None));
        assert_eq!(cache.check_with(fmt, "GET http://a\n", check), Ok(None));
        assert_eq!(runs.get(), 1);
        let error = cache.check_with(fmt, "GET !\n", check).unwrap();
        assert_eq!(error.as_deref(), Some("error by /bin/hurlfmt\n  --> -:1:1"));
        assert_eq!(cache.check_with(fmt, "GET !\n", check).unwrap(), error);
        assert_eq!(runs.get(), 2);
        // Another hurlfmt may have another opinion on the same text.
        let error = cache.check_with(other, "GET !\n", check).unwrap();
        assert_eq!(error.as_deref(), Some("error by /opt/hurlfmt\n  --> -:1:1"));
        assert_eq!(runs.get(), 3);

        // A check which could not be run is not an outcome.
        let broken =
            |_: &Path, _: &str| Err::<Option<String>, String>("cannot start hurlfmt".into());
        assert!(cache.check_with(fmt, "GET http://b\n", broken).is_err());
        assert_eq!(cache.check_with(fmt, "GET http://b\n", check), Ok(None));
        assert_eq!(runs.get(), 4);

        // Bounded: the least recently used outcomes are dropped.
        cache.forget();
        let text = |k: usize| format!("GET http://a/{k}\n");
        for k in 0..MAX_CHECKS {
            cache.check_with(fmt, &text(k), check).unwrap();
        }
        runs.set(0);
        cache.check_with(fmt, &text(0), check).unwrap();
        assert_eq!(runs.get(), 0, "still known, and now the most recent");
        cache.check_with(fmt, &text(MAX_CHECKS), check).unwrap();
        assert_eq!(
            (runs.get(), cache.checks.lock().unwrap().len()),
            (1, MAX_CHECKS)
        );
        cache.check_with(fmt, &text(0), check).unwrap();
        assert_eq!(runs.get(), 1);
        cache.check_with(fmt, &text(1), check).unwrap();
        assert_eq!(runs.get(), 2, "the oldest outcome was dropped");
        assert_eq!(cache.checks.lock().unwrap().len(), MAX_CHECKS);
    }

    #[test]
    fn syntax_is_checked_by_the_installed_hurlfmt() {
        let cache = Hurlfmt::default();
        if cache.locate("").is_none() {
            eprintln!("hurlfmt not found: test skipped");
            return;
        }
        assert_eq!(cache.syntax_error("", "GET http://a\nHTTP 200\n"), None);
        let error = cache
            .syntax_error("", "GET http://a\nHTTP 200\n[Asserts]\nstatus equalz 1\n")
            .unwrap();
        assert!(error.contains("--> -:4:"), "{error}");
        assert_eq!(cache.checks.lock().unwrap().len(), 2);
        assert_eq!(cache.syntax_error("", "GET http://a\nHTTP 200\n"), None);
        assert_eq!(cache.checks.lock().unwrap().len(), 2);
    }

    #[test]
    fn hurlfmt_which_cannot_be_started_is_searched_again() {
        let cache = Hurlfmt::default();
        let preference = "/nonexistent/hurl";
        let gone = PathBuf::from("/nonexistent/hurlfmt");
        cache
            .locations
            .lock()
            .unwrap()
            .insert(preference.into(), (Instant::now(), Some(gone)));
        // Tells nothing about the text: no error, and nothing remembered.
        assert_eq!(cache.syntax_error(preference, "GET !!\n"), None);
        assert!(cache.checks.lock().unwrap().is_empty());
        assert!(!cache.locations.lock().unwrap().contains_key(preference));
    }
}
