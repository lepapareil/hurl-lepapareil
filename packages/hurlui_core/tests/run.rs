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
//! End-to-end test of the runner: drives the real `hurl` binary against a
//! local HTTP server. Skipped when `hurl` is not installed.

use hurlui_core::{Ctx, dispatch, runner};
use serde_json::{Value, json};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct Fixture {
    ctx: Arc<Ctx>,
    root: PathBuf,
    collection: String,
    /// Value of the `X-Token` header of every request received by the server.
    tokens: Arc<Mutex<Vec<String>>>,
}

/// Size of the body of `/big`: above the 20 MiB which are loaded of a body.
const BIG: usize = 21 * 1024 * 1024;

fn header(name: &str, value: &str) -> tiny_http::Header {
    tiny_http::Header::from_bytes(name.as_bytes(), value.as_bytes()).unwrap()
}

fn fixture(tag: &str) -> Option<Fixture> {
    if runner::locate("hurl", "").is_none() {
        eprintln!("hurl not found: run test skipped");
        return None;
    }
    let root = std::env::temp_dir().join(format!("hurlui-run-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let collection = root.join("api");
    std::fs::create_dir_all(&collection).unwrap();

    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let port = server.server_addr().to_ip().unwrap().port();
    let tokens = Arc::new(Mutex::new(vec![]));
    let seen = tokens.clone();
    std::thread::spawn(move || {
        for request in server.incoming_requests() {
            if let Some(h) = request.headers().iter().find(|h| h.field.equiv("X-Token")) {
                seen.lock().unwrap().push(h.value.to_string());
            }
            let json = header("Content-Type", "application/json");
            let _ = match request.url() {
                "/user" => request.respond(
                    tiny_http::Response::from_string(r#"{"name":"bob","id":7}"#).with_header(json),
                ),
                "/gzip" => {
                    let mut enc =
                        flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
                    enc.write_all(br#"{"zipped":true}"#).unwrap();
                    request.respond(
                        tiny_http::Response::from_data(enc.finish().unwrap())
                            .with_header(json)
                            .with_header(header("Content-Encoding", "gzip")),
                    )
                }
                "/big" => request.respond(tiny_http::Response::from_data(vec![b'x'; BIG])),
                "/moved" => request.respond(
                    tiny_http::Response::from_string("moved")
                        .with_status_code(302)
                        .with_header(header("Location", "/big")),
                ),
                "/slow" => {
                    std::thread::sleep(Duration::from_secs(8));
                    request.respond(tiny_http::Response::from_string("late"))
                }
                _ => {
                    request.respond(tiny_http::Response::from_string("nope").with_status_code(404))
                }
            };
        }
    });

    let ctx = Ctx::new(root.join("config"), Box::new(|_, _| {}));
    let collection = collection.to_string_lossy().into_owned();
    dispatch(
        &ctx,
        "save_environment",
        json!({
            "collection": collection,
            "name": "local",
            "variables": [
                { "name": "base_url", "value": format!("http://127.0.0.1:{port}"), "secret": false },
                { "name": "token", "value": "s3cr3t-value", "secret": true },
            ],
        }),
    )
    .unwrap();
    Some(Fixture {
        ctx,
        root,
        collection,
        tokens,
    })
}

impl Fixture {
    fn file(&self, name: &str, content: &str) -> String {
        dispatch(
            &self.ctx,
            "create_file",
            json!({ "parent": self.collection, "name": name }),
        )
        .unwrap();
        let path = PathBuf::from(&self.collection).join(format!("{name}.hurl"));
        std::fs::write(&path, content).unwrap();
        path.to_string_lossy().into_owned()
    }

    fn run(&self, file: &str, extra: Value) -> Value {
        let mut req = json!({
            "runId": "test",
            "file": file,
            "collection": self.collection,
            "environment": "local",
            "verbosity": "verbose",
        });
        req.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        dispatch(&self.ctx, "run", req).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

const USERS: &str = r#"# Get the user
GET {{base_url}}/user
X-Token: {{token}}
HTTP 200
[Captures]
user_id: jsonpath "$.id"
[Asserts]
jsonpath "$.name" == "bob"

# Compressed
GET {{base_url}}/gzip
Accept-Encoding: gzip
HTTP 200
[Asserts]
jsonpath "$.zipped" == true

# Fails
GET {{base_url}}/missing
HTTP 200
[Asserts]
body contains "yes"
"#;

#[test]
fn runs_a_file_and_reports_entries() {
    let Some(fx) = fixture("file") else { return };
    let file = fx.file("users", USERS);
    let result = fx.run(&file, json!({ "options": { "continue-on-error": true } }));

    assert_eq!(result["success"], false);
    assert_eq!(result["cancelled"], false);
    assert_eq!(
        result["exitCode"], 4,
        "only assert errors: {}",
        result["stderr"]
    );
    let entries = result["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 3);

    // Entry 1: variables file, secret, capture, body.
    let first = &entries[0];
    assert_eq!(first["index"], 1);
    let call = &first["calls"][0];
    assert_eq!(call["response"]["status"], 200);
    assert_eq!(call["response"]["body"]["text"], r#"{"name":"bob","id":7}"#);
    assert_eq!(first["captures"][0]["name"], "user_id");
    assert_eq!(first["captures"][0]["value"], 7);
    assert!(
        first["asserts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|a| a["success"] == true)
    );
    assert!(
        first["asserts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["source"] == r#"jsonpath "$.name" == "bob""#)
    );
    assert_eq!(
        fx.tokens.lock().unwrap().first().map(String::as_str),
        Some("s3cr3t-value")
    );
    // The secret reached the server but is redacted from everything shown.
    assert!(
        !result.to_string().contains("s3cr3t-value"),
        "secret leaked in the result"
    );

    // Entry 2: the stored body is decoded for display.
    assert_eq!(
        entries[1]["calls"][0]["response"]["body"]["text"],
        r#"{"zipped":true}"#
    );

    // Entry 3: failed asserts carry their source line and message (Hurl does
    // not evaluate the other asserts once the status differs).
    let failed: Vec<&Value> = entries[2]["asserts"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|a| a["success"] == false)
        .collect();
    assert!(!failed.is_empty());
    assert_eq!(failed[0]["source"], "HTTP 200");
    assert!(failed[0]["message"].as_str().unwrap().contains("404"));
    assert!(
        result["errors"]
            .as_str()
            .unwrap()
            .contains("error: Assert status code")
    );
    assert_eq!(entries[0]["failed"], false);
    assert_eq!(entries[2]["failed"], true);
    assert_eq!(
        entries[2]["errors"],
        json!([]),
        "assert failures are not duplicated as errors"
    );
    assert!(
        result["command"]
            .as_str()
            .unwrap()
            .contains("--report-json")
    );
    assert!(
        result.get("cookies").is_none(),
        "nothing reads the cookies of the report"
    );
    let keys: Vec<&str> = result
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        [
            "cancelled",
            "command",
            "durationMs",
            "entries",
            "errors",
            "exitCode",
            "file",
            "runId",
            "stderr",
            "success"
        ]
    );
    assert_eq!(result["runId"], "test");
    assert_eq!(result["file"], file.as_str());
    assert!(result["durationMs"].is_u64());
    assert!(
        result["stderr"]
            .as_str()
            .unwrap()
            .contains("* Executing entry 1")
    );
    // Far from the budget of a run: every body is loaded.
    for entry in entries {
        assert!(
            entry["calls"][0]["response"]["body"]
                .get("omitted")
                .is_none()
        );
    }
}

#[test]
fn large_bodies_are_loaded_within_the_budget_of_the_run() {
    let Some(fx) = fixture("budget") else { return };
    // 6 bodies of 21 MiB, each truncated to 20 MiB: one more than the 100 MiB
    // loaded for a run.
    let big = "GET {{base_url}}/big\nHTTP 200\n\n";
    let file = fx.file(
        "big",
        &format!(
            "GET {{{{base_url}}}}/user\nHTTP 200\n\nGET {{{{base_url}}}}/moved\n[Options]\nlocation: true\nHTTP 200\n\n{}",
            big.repeat(5)
        ),
    );
    let result = fx.run(&file, json!({}));
    assert_eq!(result["success"], true, "{}", result["stderr"]);
    let entries = result["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 7);
    let body =
        |entry: usize, call: usize| &entries[entry - 1]["calls"][call - 1]["response"]["body"];
    let omitted = |size: usize| json!({ "size": size, "truncated": false, "encoded": false, "omitted": true });

    // The budget goes to the last entries...
    for entry in 3..=7 {
        assert_eq!(entries[entry - 1]["index"], entry);
        let body = body(entry, 1);
        assert_eq!(
            (&body["size"], &body["truncated"]),
            (&json!(BIG), &json!(true))
        );
        assert_eq!(body["text"].as_str().map(str::len), Some(20 * 1024 * 1024));
        assert!(body.get("omitted").is_none());
    }
    // ... and the bodies before them are not loaded, whatever their size:
    // the response which was redirected to, the redirection, the first entry.
    assert_eq!(entries[1]["calls"].as_array().unwrap().len(), 2);
    assert_eq!(entries[1]["calls"][0]["response"]["status"], 302);
    assert_eq!(body(2, 2), &omitted(BIG));
    assert_eq!(body(2, 1), &omitted("moved".len()));
    assert_eq!(body(1, 1), &omitted(r#"{"name":"bob","id":7}"#.len()));
    assert!(entries.iter().all(|e| e["failed"] == false));
}

#[test]
fn runs_a_single_entry() {
    let Some(fx) = fixture("entry") else { return };
    let file = fx.file("users", USERS);
    let result = fx.run(&file, json!({ "fromEntry": 2, "toEntry": 2 }));
    assert_eq!(result["success"], true, "{}", result["stderr"]);
    assert_eq!(result["exitCode"], 0);
    let entries = result["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["index"], 2);
}

#[test]
fn reports_syntax_and_runtime_errors() {
    let Some(fx) = fixture("errors") else { return };
    let file = fx.file(
        "broken",
        "GET {{base_url}}/user\nHTTP 200\n[Asserts]\njsonpath \"$.name\" equalz 1\n",
    );
    let result = fx.run(&file, json!({}));
    assert_eq!(result["success"], false);
    assert_eq!(result["exitCode"], 2);
    assert!(result["entries"].as_array().unwrap().is_empty());
    assert!(
        result["errors"].as_str().unwrap().starts_with("error:"),
        "{}",
        result["errors"]
    );

    let loaded = dispatch(&fx.ctx, "read_file", json!({ "path": file })).unwrap();
    assert!(loaded["syntaxError"].as_str().unwrap().contains("error"));
    assert_eq!(loaded["entries"].as_array().unwrap().len(), 1);
    if runner::locate_hurlfmt("").is_some() {
        // The check of a content is remembered, but each file is reported
        // under its own name.
        assert!(
            loaded["syntaxError"]
                .as_str()
                .unwrap()
                .contains("--> broken.hurl:4:"),
            "{loaded}"
        );
        let copy = fx.file("copy", &std::fs::read_to_string(&file).unwrap());
        for _ in 0..2 {
            let loaded = dispatch(&fx.ctx, "read_file", json!({ "path": copy })).unwrap();
            assert!(
                loaded["syntaxError"]
                    .as_str()
                    .unwrap()
                    .contains("--> copy.hurl:4:"),
                "{loaded}"
            );
        }
        let again = dispatch(&fx.ctx, "read_file", json!({ "path": file })).unwrap();
        assert_eq!(again["syntaxError"], loaded["syntaxError"]);
        // Looking for the binaries again forgets the checks: same answer.
        dispatch(&fx.ctx, "app_info", json!({})).unwrap();
        let again = dispatch(&fx.ctx, "read_file", json!({ "path": file })).unwrap();
        assert_eq!(again["syntaxError"], loaded["syntaxError"]);
    }

    // Runtime error which is not an assert: undefined variable.
    let file = fx.file("undefined", "GET {{base_url}}/{{undefined_variable}}\n");
    let result = fx.run(&file, json!({}));
    assert_eq!(result["success"], false);
    assert_eq!(result["exitCode"], 3);
    assert!(!result["errors"].as_str().unwrap().is_empty());
    // The runtime error is attached to its entry.
    assert_eq!(result["entries"][0]["failed"], true, "{result}");
    assert!(
        result["entries"][0]["errors"][0]
            .as_str()
            .unwrap()
            .contains("Undefined variable")
    );
}

#[test]
fn cancels_a_run() {
    let Some(fx) = fixture("cancel") else { return };
    let file = fx.file("slow", "GET {{base_url}}/slow\n");
    let ctx = fx.ctx.clone();
    let canceller = std::thread::spawn(move || {
        // Retried: the run may not be registered yet.
        for _ in 0..100 {
            std::thread::sleep(Duration::from_millis(50));
            if dispatch(&ctx, "cancel_run", json!({ "runId": "test" })).unwrap() == true {
                return true;
            }
        }
        false
    });
    let started = Instant::now();
    let result = fx.run(&file, json!({}));
    assert!(canceller.join().unwrap());
    assert_eq!(result["cancelled"], true);
    assert_eq!(result["success"], false);
    assert!(
        started.elapsed() < Duration::from_secs(6),
        "the process was not killed"
    );
}

#[test]
fn edits_entries_through_the_api() {
    let Some(fx) = fixture("edit") else { return };
    let file = fx.file("users", USERS);
    let loaded = dispatch(&fx.ctx, "read_file", json!({ "path": file })).unwrap();
    assert_eq!(loaded["syntaxError"], Value::Null);
    let mut entry = loaded["entries"][2].clone();
    entry["url"] = json!("{{base_url}}/user");
    entry["asserts"] = json!([{ "query": "jsonpath \"$.id\"", "predicate": "==", "value": "7" }]);
    let saved = dispatch(
        &fx.ctx,
        "save_entry",
        json!({ "path": file, "index": 3, "entry": entry }),
    )
    .unwrap();
    assert_eq!(saved["syntaxError"], Value::Null);
    assert!(
        saved["raw"]
            .as_str()
            .unwrap()
            .starts_with(&USERS[..USERS.find("# Fails").unwrap()])
    );

    let result = fx.run(&file, json!({}));
    assert_eq!(result["success"], true, "{}", result["stderr"]);

    let added = dispatch(
        &fx.ctx,
        "add_entry",
        json!({ "path": file, "entry": { "method": "GET", "url": "{{base_url}}/user", "docs": "Again" } }),
    )
    .unwrap();
    assert_eq!(added["entries"].as_array().unwrap().len(), 4);
    let deleted = dispatch(&fx.ctx, "delete_entry", json!({ "path": file, "index": 1 })).unwrap();
    assert_eq!(deleted["entries"].as_array().unwrap().len(), 3);
    assert_eq!(deleted["entries"][2]["docs"], "Again");

    let scanned = dispatch(&fx.ctx, "scan_collection", json!({ "path": fx.collection })).unwrap();
    assert_eq!(
        scanned["children"][0]["entries"].as_array().unwrap().len(),
        3
    );
    assert_eq!(scanned["environments"][0]["name"], "local");

    if runner::locate_hurlfmt("").is_some() {
        let entry = dispatch(
            &fx.ctx,
            "curl_to_entry",
            json!({ "curl": "curl -X POST 'http://localhost:8000/a' \\\n  -H 'Content-Type: application/json' -d '{\"a\":1}'" }),
        )
        .unwrap();
        assert_eq!(entry["method"], "POST");
        assert_eq!(entry["url"], "http://localhost:8000/a");
        assert_eq!(entry["headers"][0]["name"], "Content-Type");
    }
}
