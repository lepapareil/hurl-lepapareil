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
//! Command dispatcher shared by the Tauri shell and the development bridge.
//!
//! Every command takes a JSON object and returns a JSON value, so both
//! transports expose exactly the same API to the frontend.

use notify::RecommendedWatcher;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::collections::hash_map::Entry as MapEntry;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::collection::{self, HurlFile, ScanCache, Variable};
use crate::hurlfile::{self, Entry};
use crate::runner::{self, Hurlfmt, RunRequest, Runs};
use crate::{options, store, watcher};

type Emitter = Box<dyn Fn(&str, Value) + Send + Sync>;

pub struct Ctx {
    pub config_dir: PathBuf,
    emit: Emitter,
    runs: Runs,
    watchers: Mutex<HashMap<String, RecommendedWatcher>>,
    /// What the last scan of each open collection (by path) learnt of its
    /// files: a collection is scanned again after every change.
    scans: Mutex<HashMap<String, ScanCache>>,
    /// Location of `hurlfmt` and outcome of its recent syntax checks.
    hurlfmt: Hurlfmt,
}

impl Ctx {
    /// `emit` pushes an event (name, payload) to the frontend.
    pub fn new(config_dir: PathBuf, emit: Emitter) -> Arc<Ctx> {
        // Run directories left by an application which was killed during a
        // run. In the background: the start of the application does not wait
        // for the scan of the temporary directory. Best effort, as is the
        // start of the thread itself.
        let _ = std::thread::Builder::new()
            .name("stale-run-dirs".into())
            .spawn(runner::sweep_stale_run_dirs);
        Arc::new(Ctx {
            config_dir,
            emit,
            runs: Runs::default(),
            watchers: Mutex::new(HashMap::new()),
            scans: Mutex::new(HashMap::new()),
            hurlfmt: Hurlfmt::default(),
        })
    }
}

fn arg<T: DeserializeOwned>(args: &Value, name: &str) -> Result<T, String> {
    let value = args
        .get(name)
        .ok_or_else(|| format!("missing argument: {name}"))?;
    serde_json::from_value(value.clone()).map_err(|e| format!("invalid argument {name}: {e}"))
}

fn opt<T: DeserializeOwned>(args: &Value, name: &str) -> Result<Option<T>, String> {
    match args.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => serde_json::from_value(value.clone())
            .map(Some)
            .map_err(|e| format!("invalid argument {name}: {e}")),
    }
}

fn to_value<T: serde::Serialize>(value: T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|e| e.to_string())
}

fn hurl_path(args: &Value) -> String {
    args.get("hurlPath")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn load_file(ctx: &Ctx, path: &str, hurl_path: &str) -> Result<HurlFile, String> {
    // Taken before the file is read (see `file_stamp`).
    let stamp = collection::file_stamp(Path::new(path));
    let raw = collection::read_text(path)?;
    // hurlfmt reads the text from its standard input and names it `-`. The
    // message may come from the check of another file with the same content:
    // the name is put in afterwards.
    let name = Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let syntax_error = ctx
        .hurlfmt
        .syntax_error(hurl_path, &raw)
        .map(|message| message.replace("--> -:", &format!("--> {name}:")));
    Ok(HurlFile {
        path: path.to_string(),
        stamp,
        entries: hurlfile::parse(&raw),
        syntax_error,
        raw,
    })
}

pub fn dispatch(ctx: &Arc<Ctx>, cmd: &str, args: Value) -> Result<Value, String> {
    let config = ctx.config_dir.as_path();
    match cmd {
        "app_info" => {
            // The GUI asks where the binaries are at startup and when the
            // preference changes: nothing known about them is trusted then.
            ctx.hurlfmt.forget();
            Ok(runner::app_info(&hurl_path(&args)))
        }
        // Options of `hurl`, shared with Hurl: see `options`.
        "hurl_options" => to_value(options::catalog()),
        // Predicates of an assert, checked against `hurl_core`.
        "hurl_predicates" => to_value(crate::hurlfile::PREDICATES),
        "load_state" => store::load_state(config),
        "save_state" => {
            store::save_state(config, args.get("state").unwrap_or(&Value::Null))?;
            Ok(Value::Null)
        }

        // Collections
        "scan_collection" => {
            let path: String = arg(&args, "path")?;
            // Taken out for the time of the scan: the lock is not held while
            // files are read. A collection which cannot be scanned any more
            // loses its cache.
            let mut cache = ctx.scans.lock().unwrap().remove(&path).unwrap_or_default();
            let collection = collection::scan(config, &path, &mut cache)?;
            ctx.scans.lock().unwrap().insert(path, cache);
            to_value(collection)
        }
        "create_collection" => to_value(collection::create_collection(
            &arg::<String>(&args, "parent")?,
            &arg::<String>(&args, "name")?,
        )?),
        "watch_collection" => {
            let path: String = arg(&args, "path")?;
            let mut watchers = ctx.watchers.lock().unwrap();
            if let MapEntry::Vacant(slot) = watchers.entry(path.clone()) {
                let emitter = ctx.clone();
                let payload = json!({ "path": path });
                let watcher = watcher::watch(Path::new(&path), move || {
                    (emitter.emit)("collection-changed", payload.clone());
                })?;
                slot.insert(watcher);
            }
            Ok(Value::Null)
        }
        "unwatch_collection" => {
            // The collection is being closed: nothing is kept about it.
            let path: String = arg(&args, "path")?;
            ctx.watchers.lock().unwrap().remove(&path);
            ctx.scans.lock().unwrap().remove(&path);
            Ok(Value::Null)
        }

        // Files and folders
        "create_folder" => to_value(collection::create_folder(
            &arg::<String>(&args, "parent")?,
            &arg::<String>(&args, "name")?,
        )?),
        "create_file" => to_value(collection::create_file(
            &arg::<String>(&args, "parent")?,
            &arg::<String>(&args, "name")?,
            opt::<Entry>(&args, "entry")?.as_ref(),
        )?),
        "rename_path" => to_value(collection::rename_path(
            &arg::<String>(&args, "path")?,
            &arg::<String>(&args, "name")?,
        )?),
        "clone_file" => to_value(collection::clone_file(
            &arg::<String>(&args, "path")?,
            &arg::<String>(&args, "name")?,
        )?),
        "delete_path" => {
            collection::delete_path(&arg::<String>(&args, "path")?)?;
            Ok(Value::Null)
        }
        "body_file_path" => {
            let (path, outside) = collection::body_file_path(
                &arg::<String>(&args, "file")?,
                &arg::<String>(&args, "target")?,
            );
            Ok(json!({ "path": path, "outside": outside }))
        }
        "reveal_path" => {
            collection::reveal(&arg::<String>(&args, "path")?)?;
            Ok(Value::Null)
        }

        // Hurl files
        "read_file" => to_value(load_file(
            ctx,
            &arg::<String>(&args, "path")?,
            &hurl_path(&args),
        )?),
        "save_raw" => {
            let path: String = arg(&args, "path")?;
            collection::write_text(&path, &arg::<String>(&args, "raw")?)?;
            to_value(load_file(ctx, &path, &hurl_path(&args))?)
        }
        "save_entry" => {
            let path: String = arg(&args, "path")?;
            let text = collection::read_text(&path)?;
            let text = hurlfile::replace_entry(
                &text,
                arg::<usize>(&args, "index")?,
                &arg::<Entry>(&args, "entry")?,
            )?;
            collection::write_text(&path, &text)?;
            to_value(load_file(ctx, &path, &hurl_path(&args))?)
        }
        "add_entry" => {
            let path: String = arg(&args, "path")?;
            let text = collection::read_text(&path)?;
            let text = hurlfile::insert_entry(
                &text,
                opt::<usize>(&args, "after")?,
                &arg::<Entry>(&args, "entry")?,
            )?;
            collection::write_text(&path, &text)?;
            to_value(load_file(ctx, &path, &hurl_path(&args))?)
        }
        "delete_entry" => {
            let path: String = arg(&args, "path")?;
            let text = collection::read_text(&path)?;
            let text = hurlfile::delete_entry(&text, arg::<usize>(&args, "index")?)?;
            collection::write_text(&path, &text)?;
            to_value(load_file(ctx, &path, &hurl_path(&args))?)
        }
        "curl_to_entry" => {
            let hurlfmt = ctx.hurlfmt.locate(&hurl_path(&args));
            let text = runner::curl_to_hurl(hurlfmt.as_deref(), &arg::<String>(&args, "curl")?)?;
            let entry = hurlfile::parse(&text)
                .into_iter()
                .next()
                .ok_or("no request found in the curl command")?;
            to_value(entry)
        }

        // Environments
        "create_environment" => {
            collection::create_environment(
                config,
                &arg::<String>(&args, "collection")?,
                &arg::<String>(&args, "name")?,
            )?;
            Ok(Value::Null)
        }
        "save_environment" => {
            collection::save_environment(
                config,
                &arg::<String>(&args, "collection")?,
                &arg::<String>(&args, "name")?,
                &arg::<Vec<Variable>>(&args, "variables")?,
            )?;
            Ok(Value::Null)
        }
        "delete_environment" => {
            collection::delete_environment(
                config,
                &arg::<String>(&args, "collection")?,
                &arg::<String>(&args, "name")?,
            )?;
            Ok(Value::Null)
        }
        "rename_environment" => {
            collection::rename_environment(
                config,
                &arg::<String>(&args, "collection")?,
                &arg::<String>(&args, "name")?,
                &arg::<String>(&args, "newName")?,
            )?;
            Ok(Value::Null)
        }

        // Execution
        "run" => {
            let req: RunRequest =
                serde_json::from_value(args).map_err(|e| format!("invalid run request: {e}"))?;
            let secrets = match req.environment.as_deref() {
                Some(env) if !env.is_empty() => store::secrets(config, &req.collection, env),
                _ => vec![],
            };
            runner::run(&ctx.runs, &secrets, &req)
        }
        "cancel_run" => Ok(Value::Bool(
            ctx.runs.cancel(&arg::<String>(&args, "runId")?),
        )),

        _ => Err(format!("unknown command: {cmd}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{Duration, SystemTime};

    #[test]
    fn hurl_options_are_the_ones_of_the_catalog() {
        let ctx = Ctx::new(std::env::temp_dir(), Box::new(|_, _| {}));
        let catalog = dispatch(&ctx, "hurl_options", json!({})).unwrap();
        assert!(catalog.get("comment").is_none());
        assert_eq!(catalog["headings"][0], "HTTP options");
        let options = catalog["options"].as_array().unwrap();
        let option = |long: &str| options.iter().find(|o| o["long"] == long).unwrap();
        assert_eq!(option("insecure")["kind"], "flag");
        assert_eq!(option("insecure")["managed"], false);
        assert_eq!(option("insecure")["section"], true);
        assert_eq!(option("max-time")["default"], "300");
        assert_eq!(option("report-json")["managed"], true);
        assert_eq!(option("skip")["cli"], false);
        assert_eq!(option("skip")["section_kind"], "boolean");
        assert_eq!(option("max-time")["section_kind"], "duration");
        assert_eq!(option("retry")["section_min"], -1);
        assert_eq!(option("report-json")["section_kind"], Value::Null);
        assert_eq!(catalog["duration_units"][0], "ms");

        let predicates = dispatch(&ctx, "hurl_predicates", json!({})).unwrap();
        let predicates = predicates.as_array().unwrap();
        assert!(predicates.contains(&json!("includes")));
        assert!(!predicates.contains(&json!("equals")));
    }

    #[test]
    fn scan_cache_lives_as_long_as_the_collection_is_open() {
        let root = std::env::temp_dir().join(format!("hurlui-api-scan-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let collection = root.join("api");
        fs::create_dir_all(&collection).unwrap();
        let file = collection.join("a.hurl");
        let old = SystemTime::now() - Duration::from_secs(3600);
        let write = |text: &str| {
            fs::write(&file, text).unwrap();
            fs::File::options()
                .write(true)
                .open(&file)
                .unwrap()
                .set_modified(old)
                .unwrap();
        };
        let ctx = Ctx::new(root.join("config"), Box::new(|_, _| {}));
        let path = collection.to_string_lossy().into_owned();
        let scan = || dispatch(&ctx, "scan_collection", json!({ "path": path }));
        let url = |scanned: Value| scanned["children"][0]["entries"][0]["url"].clone();

        write("GET http://one\n");
        assert_eq!(url(scan().unwrap()), "http://one");
        // Same size and modification time: the file is not read again by the
        // next command.
        write("GET http://two\n");
        assert_eq!(url(scan().unwrap()), "http://one");
        assert!(ctx.scans.lock().unwrap().contains_key(&path));

        // Closing the collection forgets it.
        dispatch(&ctx, "unwatch_collection", json!({ "path": path })).unwrap();
        assert!(ctx.scans.lock().unwrap().is_empty());
        assert_eq!(url(scan().unwrap()), "http://two");
        // So does a collection which is gone.
        fs::remove_dir_all(&collection).unwrap();
        assert!(scan().is_err());
        assert!(ctx.scans.lock().unwrap().is_empty());
        fs::remove_dir_all(&root).unwrap();
    }
}
