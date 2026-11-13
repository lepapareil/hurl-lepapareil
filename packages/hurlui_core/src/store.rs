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
//! Persistence of the application state and of the environment secrets, as
//! JSON files in the application configuration directory.

use serde_json::{Map, Value};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::hurlfile::KeyValue;

/// `Ok(None)` when the file does not exist. A file which cannot be read or
/// parsed is an error, never "no data": treating it as empty would let the
/// next save replace everything it holds.
fn read_json(path: &Path) -> Result<Option<Value>, String> {
    match fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text)
            .map(Some)
            .map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Creates the configuration directory when it does not exist: it holds the
/// secrets, so it is only made accessible to the current user. An existing
/// directory is left as it is.
fn create_config_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
    }
    // Elsewhere the directory is protected by the access rights of the user
    // profile it lives in.
    #[cfg(not(unix))]
    {
        fs::create_dir_all(dir)
    }
}

/// Creates a new temporary file next to `target`.
///
/// The name is unique (process and counter): two running instances do not
/// write in each other's file. The file is always a new one, so an existing
/// file or a symbolic link planted under that name is never written through,
/// and a `private` file is born readable by the current user only instead of
/// being restricted after the fact.
fn create_temp(target: &Path, private: bool) -> Result<(PathBuf, File), String> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        if private {
            options.mode(0o600);
        }
    }
    // No mode on other systems: the file gets the access rights of the user
    // profile.
    #[cfg(not(unix))]
    let _ = private;
    let mut attempts = 0;
    loop {
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp = target.with_file_name(format!("{name}.{}.{unique}.tmp", std::process::id()));
        match options.open(&tmp) {
            Ok(file) => return Ok((tmp, file)),
            // Left behind by a former process which had the same identifier.
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists && attempts < 100 => attempts += 1,
            Err(e) => return Err(format!("{}: {e}", tmp.display())),
        }
    }
}

/// Writes through a temporary file so that a crash cannot leave a truncated
/// file behind. `private` files (secrets) are never readable by other users,
/// not even while they are being written.
fn write_json(path: &Path, value: &Value, private: bool) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        create_config_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    let (tmp, mut file) = create_temp(path, private)?;
    let written = file
        .write_all(text.as_bytes())
        .map_err(|e| format!("{}: {e}", tmp.display()));
    // Closed before being renamed: Windows refuses to move an open file.
    drop(file);
    let result = written
        .and_then(|()| fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display())));
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn state_path(config_dir: &Path) -> PathBuf {
    config_dir.join("state.json")
}

fn secrets_path(config_dir: &Path) -> PathBuf {
    config_dir.join("secrets.json")
}

/// The state is an opaque JSON document owned by the frontend (opened
/// collections, preferences, active environments...).
pub fn load_state(config_dir: &Path) -> Result<Value, String> {
    let path = state_path(config_dir);
    read_json(&path)
        .map(|state| state.unwrap_or(Value::Null))
        .map_err(|e| {
            // The application starts with default settings and will save them:
            // the unreadable file is kept aside instead of being lost.
            let backup = path.with_extension("json.invalid");
            match fs::copy(&path, &backup) {
                Ok(_) => format!(
                    "{e}. Default settings are used; the file was copied to {}",
                    backup.display()
                ),
                Err(_) => format!("{e}. Default settings are used"),
            }
        })
}

pub fn save_state(config_dir: &Path, state: &Value) -> Result<(), String> {
    write_json(&state_path(config_dir), state, false)
}

/// Secrets are kept out of the collection (which is usually versioned), in
/// `secrets.json`: `{ "<collection path>": { "<environment>": { name: value } } }`.
pub fn secrets(config_dir: &Path, collection: &str, environment: &str) -> Vec<KeyValue> {
    let Ok(Some(all)) = read_json(&secrets_path(config_dir)) else {
        return vec![];
    };
    all.get(collection)
        .and_then(|c| c.get(environment))
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .map(|(name, value)| KeyValue {
                    name: name.clone(),
                    value: value.as_str().unwrap_or_default().to_string(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Fails when the secrets file exists but cannot be read: environments must
/// not be modified then, the file would be rewritten without its content.
pub fn check_secrets(config_dir: &Path) -> Result<(), String> {
    read_json(&secrets_path(config_dir))
        .map(|_| ())
        .map_err(|e| format!("{e}. Fix or remove this file before editing environments"))
}

pub fn set_secrets(
    config_dir: &Path,
    collection: &str,
    environment: &str,
    secrets: Option<&[KeyValue]>,
) -> Result<(), String> {
    let path = secrets_path(config_dir);
    let mut all = match read_json(&path)? {
        Some(Value::Object(m)) => m,
        Some(_) => return Err(format!("{}: not a JSON object", path.display())),
        None => Map::new(),
    };
    let envs = all
        .entry(collection.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if let Some(envs) = envs.as_object_mut() {
        match secrets.filter(|s| !s.is_empty()) {
            Some(secrets) => {
                let map: Map<String, Value> = secrets
                    .iter()
                    .map(|kv| (kv.name.clone(), Value::String(kv.value.clone())))
                    .collect();
                envs.insert(environment.to_string(), Value::Object(map));
            }
            None => {
                envs.remove(environment);
            }
        }
    }
    if all
        .get(collection)
        .and_then(Value::as_object)
        .is_some_and(Map::is_empty)
    {
        all.remove(collection);
    }
    if all.is_empty() && !path.exists() {
        return Ok(());
    }
    write_json(&path, &Value::Object(all), true)
}

pub fn rename_secrets(
    config_dir: &Path,
    collection: &str,
    from: &str,
    to: &str,
) -> Result<(), String> {
    let current = secrets(config_dir, collection, from);
    if current.is_empty() {
        return Ok(());
    }
    set_secrets(config_dir, collection, to, Some(&current))?;
    set_secrets(config_dir, collection, from, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hurlui-store-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn unreadable_secrets_are_never_overwritten() {
        let dir = temp_dir("secrets");
        let one = [KeyValue {
            name: "token".into(),
            value: "abc".into(),
        }];
        set_secrets(&dir, "/c", "local", Some(&one)).unwrap();
        assert_eq!(secrets(&dir, "/c", "local"), one);

        // A typo in the file must not cost the secrets of every collection.
        let broken = "{ \"/c\": { \"local\": { \"token\": \"abc\" } }";
        fs::write(secrets_path(&dir), broken).unwrap();
        assert!(check_secrets(&dir).is_err());
        assert!(set_secrets(&dir, "/other", "dev", Some(&one)).is_err());
        assert!(set_secrets(&dir, "/c", "local", None).is_err());
        assert_eq!(fs::read_to_string(secrets_path(&dir)).unwrap(), broken);
        fs::remove_dir_all(&dir).unwrap();
    }

    fn file_names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[cfg(unix)]
    fn mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        fs::symlink_metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[cfg(unix)]
    #[test]
    fn secrets_are_private_from_their_creation() {
        let root = temp_dir("private");
        // The configuration directory does not exist yet.
        let dir = root.join("config").join("hurlui");
        let one = [KeyValue {
            name: "token".into(),
            value: "abc".into(),
        }];
        set_secrets(&dir, "/c", "local", Some(&one)).unwrap();
        assert_eq!(mode(&secrets_path(&dir)), 0o600);
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(file_names(&dir), ["secrets.json"]);

        // A file which was readable by others is replaced by a private one,
        // and the mode of an existing directory is not touched.
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(secrets_path(&dir), fs::Permissions::from_mode(0o644)).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o750)).unwrap();
        set_secrets(&dir, "/c", "dev", Some(&one)).unwrap();
        assert_eq!(mode(&secrets_path(&dir)), 0o600);
        assert_eq!(mode(&dir), 0o750);
        assert_eq!(secrets(&dir, "/c", "local"), one);
        assert_eq!(secrets(&dir, "/c", "dev"), one);

        // The state is not a secret: usual permissions, but no leftover.
        save_state(&dir, &serde_json::json!({ "a": 1 })).unwrap();
        save_state(&dir, &serde_json::json!({ "a": 2 })).unwrap();
        assert_eq!(load_state(&dir).unwrap()["a"], 2);
        assert_eq!(file_names(&dir), ["secrets.json", "state.json"]);
        fs::remove_dir_all(&root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn planted_temporary_file_is_not_followed() {
        let dir = temp_dir("symlink");
        let victim = dir.join("victim.txt");
        fs::write(&victim, "untouched").unwrap();
        // Name of the temporary file of former versions: predictable.
        let planted = dir.join("secrets.json.tmp");
        std::os::unix::fs::symlink(&victim, &planted).unwrap();

        let one = [KeyValue {
            name: "token".into(),
            value: "abc".into(),
        }];
        set_secrets(&dir, "/c", "local", Some(&one)).unwrap();
        assert_eq!(secrets(&dir, "/c", "local"), one);
        assert_eq!(mode(&secrets_path(&dir)), 0o600);
        assert_eq!(fs::read_to_string(&victim).unwrap(), "untouched");
        assert!(
            fs::symlink_metadata(&planted)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(
            fs::symlink_metadata(secrets_path(&dir))
                .unwrap()
                .file_type()
                .is_file()
        );
        assert_eq!(
            file_names(&dir),
            ["secrets.json", "secrets.json.tmp", "victim.txt"]
        );

        // Same when the link is where the next temporary file would be: the
        // write goes to another name instead of through the link.
        let next = create_temp(&secrets_path(&dir), true).unwrap().0;
        fs::remove_file(&next).unwrap();
        let (stem, counter) = {
            let name = next.file_name().unwrap().to_string_lossy().into_owned();
            let name = name.strip_suffix(".tmp").unwrap().to_string();
            let (stem, counter) = name.rsplit_once('.').unwrap();
            (stem.to_string(), counter.parse::<u64>().unwrap())
        };
        // Other tests of this process draw from the same counter.
        let links: Vec<PathBuf> = (counter + 1..counter + 40)
            .map(|k| dir.join(format!("{stem}.{k}.tmp")))
            .collect();
        for link in &links {
            std::os::unix::fs::symlink(&victim, link).unwrap();
        }
        set_secrets(&dir, "/c", "dev", Some(&one)).unwrap();
        assert_eq!(secrets(&dir, "/c", "dev"), one);
        assert_eq!(fs::read_to_string(&victim).unwrap(), "untouched");
        assert!(
            links
                .iter()
                .all(|l| fs::symlink_metadata(l).unwrap().file_type().is_symlink())
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn failed_write_leaves_no_temporary_file() {
        let dir = temp_dir("failed");
        // The target cannot be replaced: a directory holds its name.
        fs::create_dir(state_path(&dir)).unwrap();
        fs::write(state_path(&dir).join("keep"), "").unwrap();
        assert!(save_state(&dir, &serde_json::json!({ "a": 1 })).is_err());
        assert_eq!(file_names(&dir), ["state.json"]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn invalid_state_is_reported_and_kept_aside() {
        let dir = temp_dir("state");
        assert_eq!(load_state(&dir), Ok(Value::Null));
        save_state(&dir, &serde_json::json!({ "collections": ["/c"] })).unwrap();
        assert_eq!(load_state(&dir).unwrap()["collections"][0], "/c");

        fs::write(state_path(&dir), "{ not json").unwrap();
        let error = load_state(&dir).unwrap_err();
        assert!(error.contains("state.json.invalid"), "{error}");
        assert_eq!(
            fs::read_to_string(dir.join("state.json.invalid")).unwrap(),
            "{ not json"
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}
