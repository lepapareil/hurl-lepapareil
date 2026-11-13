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
//! Oracle test: compares the tolerant reader/writer of Hurl UI with the
//! reference parser (`hurlfmt --out json`) on a corpus of Hurl files.
//!
//! The test is skipped unless `HURL_CORPUS` points to a directory of `*.hurl`
//! files, for instance the `integration` directory of the Hurl repository:
//!
//! ```sh
//! HURL_CORPUS=/path/to/hurl/integration cargo test -p hurlui_core --test corpus -- --nocapture
//! ```

use hurlui_core::hurlfile::{self, Entry, KeyValue};
use hurlui_core::runner;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

fn hurl_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            hurl_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "hurl") {
            out.push(path);
        }
    }
}

fn reference(hurlfmt: &Path, file: &Path) -> Option<Value> {
    let out = Command::new(hurlfmt)
        .args(["--no-color", "--out", "json"])
        .arg(file)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    serde_json::from_slice(&out.stdout).ok()
}

/// The model keeps raw source text while the reference decodes escapes and
/// normalizes templates (`{{  url }}`): such values are not compared literally.
fn comparable(raw: &str) -> bool {
    !raw.contains('\\') && !raw.contains("{{ ")
}

fn pairs(value: &Value, key: &str) -> Vec<(String, String)> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|kv| {
                    (
                        kv["name"].as_str().unwrap_or_default().to_string(),
                        kv["value"].as_str().unwrap_or_default().to_string(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

fn len(value: &Value, key: &str) -> usize {
    value.get(key).and_then(Value::as_array).map_or(0, Vec::len)
}

fn check_pairs(what: &str, mine: &[KeyValue], theirs: &[(String, String)]) -> Result<(), String> {
    if mine.len() != theirs.len() {
        return Err(format!(
            "{what}: {} instead of {}",
            mine.len(),
            theirs.len()
        ));
    }
    for (m, t) in mine.iter().zip(theirs) {
        if comparable(&m.name) && m.name != t.0 {
            return Err(format!("{what} name: {:?} instead of {:?}", m.name, t.0));
        }
        if comparable(&m.value) && m.value != t.1 {
            return Err(format!("{what} value: {:?} instead of {:?}", m.value, t.1));
        }
    }
    Ok(())
}

fn check_entry(mine: &Entry, theirs: &Value) -> Result<(), String> {
    let req = &theirs["request"];
    if mine.method != req["method"].as_str().unwrap_or_default() {
        return Err(format!(
            "method: {:?} instead of {}",
            mine.method, req["method"]
        ));
    }
    if comparable(&mine.url) && mine.url != req["url"].as_str().unwrap_or_default() {
        return Err(format!("url: {:?} instead of {}", mine.url, req["url"]));
    }
    check_pairs("header", &mine.headers, &pairs(req, "headers"))?;
    check_pairs("query", &mine.query, &pairs(req, "query_string_params"))?;
    check_pairs("form", &mine.form, &pairs(req, "form_params"))?;
    check_pairs("cookie", &mine.cookies, &pairs(req, "cookies"))?;
    if mine.multipart.len() != len(req, "multipart_form_data") {
        return Err("multipart count".into());
    }
    if mine.options.len() != len(req, "options") {
        return Err(format!(
            "options: {} instead of {}",
            mine.options.len(),
            len(req, "options")
        ));
    }
    let has_body = !matches!(mine.body.mode.as_str(), "none" | "form" | "multipart");
    if has_body != req.get("body").is_some() {
        return Err(format!("request body presence: mode {}", mine.body.mode));
    }
    let resp = theirs.get("response");
    if mine.has_response != resp.is_some() {
        return Err("response presence".into());
    }
    if let Some(resp) = resp {
        let status = resp
            .get("status")
            .map_or("*".to_string(), |s| s.to_string());
        if mine.status != status {
            return Err(format!("status: {:?} instead of {status}", mine.status));
        }
        let version = resp
            .get("version")
            .and_then(Value::as_str)
            .unwrap_or("HTTP");
        if mine.version != version {
            return Err(format!("version: {:?} instead of {version}", mine.version));
        }
        check_pairs(
            "response header",
            &mine.response_headers,
            &pairs(resp, "headers"),
        )?;
        let captures: Vec<String> = resp
            .get("captures")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .map(|c| c["name"].as_str().unwrap_or_default().to_string())
                    .collect()
            })
            .unwrap_or_default();
        let mine_captures: Vec<String> = mine.captures.iter().map(|c| c.name.clone()).collect();
        if mine_captures.len() != captures.len()
            || mine_captures
                .iter()
                .zip(&captures)
                .any(|(m, t)| comparable(m) && m != t)
        {
            return Err(format!(
                "captures: {mine_captures:?} instead of {captures:?}"
            ));
        }
        if mine.asserts.len() != len(resp, "asserts") {
            return Err(format!(
                "asserts: {} instead of {}",
                mine.asserts.len(),
                len(resp, "asserts")
            ));
        }
        if mine
            .asserts
            .iter()
            .any(|a| a.query.is_empty() || a.predicate.is_empty())
        {
            return Err("assert without query or predicate".into());
        }
        if mine.response_body.is_empty() == resp.get("body").is_some() {
            return Err("response body presence".into());
        }
    }
    Ok(())
}

fn strip_comments(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("comments");
            map.values_mut().for_each(strip_comments);
        }
        Value::Array(items) => items.iter_mut().for_each(strip_comments),
        _ => {}
    }
}

#[test]
fn agrees_with_hurlfmt_on_corpus() {
    let Some(corpus) = std::env::var_os("HURL_CORPUS") else {
        eprintln!("HURL_CORPUS not set: corpus test skipped");
        return;
    };
    let Some(hurlfmt) = runner::locate_hurlfmt("") else {
        eprintln!("hurlfmt not found: corpus test skipped");
        return;
    };
    let mut files = vec![];
    hurl_files(Path::new(&corpus), &mut files);
    files.sort();
    assert!(!files.is_empty(), "no hurl file in the corpus");

    let tmp = std::env::temp_dir().join(format!("hurlui-corpus-{}.hurl", std::process::id()));
    let (mut checked, mut invalid) = (0, 0);
    let mut failures: Vec<String> = vec![];
    for file in &files {
        let Ok(text) = std::fs::read_to_string(file) else {
            invalid += 1;
            continue;
        };
        let Some(mut expected) = reference(&hurlfmt, file) else {
            invalid += 1;
            continue;
        };
        checked += 1;
        let entries = hurlfile::parse(&text);
        let theirs = expected["entries"].as_array().cloned().unwrap_or_default();

        // 1. Reading: same structure as the reference parser.
        if entries.len() != theirs.len() {
            failures.push(format!(
                "{}: {} entries instead of {}",
                file.display(),
                entries.len(),
                theirs.len()
            ));
            continue;
        }
        if let Some(e) = entries.iter().zip(&theirs).find_map(|(m, t)| {
            check_entry(m, t)
                .err()
                .map(|e| format!("entry {}: {e}", m.index))
        }) {
            failures.push(format!("{}: {e}", file.display()));
            continue;
        }

        // The output check must not refuse entries of valid files.
        if let Some(e) = entries.iter().find_map(|e| {
            hurlfile::checked_entry(e, "\n")
                .err()
                .map(|err| format!("entry {}: {err}", e.index))
        }) {
            failures.push(format!("{}: refused: {e}", file.display()));
            continue;
        }

        // 2. Writing: a file regenerated from the model is equivalent.
        let regenerated = entries
            .iter()
            .map(|e| hurlfile::serialize_entry(e, "\n"))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&tmp, &regenerated).unwrap();
        match reference(&hurlfmt, &tmp) {
            None => failures.push(format!(
                "{}: regenerated file does not parse",
                file.display()
            )),
            Some(mut actual) => {
                strip_comments(&mut expected);
                strip_comments(&mut actual);
                if actual != expected {
                    failures.push(format!("{}: regenerated file differs", file.display()));
                }
            }
        }

        // 3. Splicing an untouched entry must not change the meaning either.
        if let Some(last) = entries.last() {
            let spliced = hurlfile::replace_entry(&text, last.index, last).unwrap();
            let again = hurlfile::parse(&spliced);
            if again.len() != entries.len() {
                failures.push(format!(
                    "{}: splice changed the entry count",
                    file.display()
                ));
            }
        }
    }
    let _ = std::fs::remove_file(&tmp);
    eprintln!(
        "corpus: {} files, {checked} checked, {invalid} rejected by hurlfmt, {} failures",
        files.len(),
        failures.len()
    );
    for f in &failures {
        eprintln!("  {f}");
    }
    assert!(failures.is_empty(), "{} corpus failures", failures.len());
}
