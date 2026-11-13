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
//! Development bridge: exposes the core API over HTTP on localhost so that the
//! frontend can run in a regular browser (`npm run bridge` + `npm run dev`),
//! without the Tauri shell. Useful for UI work and for browser-driven tests.
//!
//! ```sh
//! cargo run -p hurlui_core --example dev_bridge
//! ```
//!
//! The bridge prints the URL to open, `http://localhost:1420/#bridge-token=<token>`.
//!
//! This is a development tool only: its API reads and writes files and starts
//! programs, and it listens on a port which every local process, other users
//! of the machine included, can reach, just like the port of the Vite
//! development server in front of it. Access is therefore restricted to the
//! holder of a token:
//!
//! - a random token is generated at every launch. It is only known to the
//!   page opened from the printed URL, which takes it from the fragment (never
//!   sent to a server), keeps it for the session of the tab and sends it in a
//!   header of every request, and to the processes of the current user,
//!   through a file that only this user can read (`target/dev-bridge.token`),
//!   meant for automation and tests;
//! - the Vite proxy forwards the requests of the page and adds nothing: being
//!   able to reach the Vite port, or this one, gives no access. Other users
//!   of the machine and web pages reaching the ports directly (DNS rebinding
//!   included) do not have the token;
//! - requests carrying an `Origin` which is not the development server are
//!   refused, so a page of another site cannot use the bridge either, even
//!   with the token.
//!
//! The state of the application (`state.json`, whose `hurlPath` preference is
//! a program the core starts, and `secrets.json`) is kept apart from the one
//! of the desktop application, in `target/dev-bridge-config`: inside the
//! checkout, which belongs to the developer, and not under a fixed name in
//! the temporary directory, where another user of the machine could create
//! the directory first.
//!
//! Environment variables: `HURLUI_BRIDGE_PORT` (1421), `HURLUI_CONFIG_DIR`,
//! `HURLUI_BRIDGE_TOKEN_FILE` and `HURLUI_BRIDGE_ORIGINS` (comma separated;
//! the first one is the address of the frontend).

use hurlui_core::{Ctx, dispatch};
use serde_json::{Value, json};
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const TOKEN_HEADER: &str = "X-Hurlui-Bridge-Token";
const DEFAULT_ORIGINS: &str = "http://localhost:1420,http://127.0.0.1:1420";

/// 64 hexadecimal digits derived from the random keys of `RandomState`,
/// which the standard library asks the operating system for: 128 bits which
/// cannot be guessed, without any dependency.
fn random_token() -> String {
    (0..4u8)
        .map(|i| {
            let mut hasher = RandomState::new().build_hasher();
            hasher.write_u8(i);
            format!("{:016x}", hasher.finish())
        })
        .collect()
}

/// Compares the token of a request with the expected one in constant time:
/// the time taken must not tell how many of its first characters are right.
/// The length is not a secret (all tokens have the same).
fn same_token(given: &str, expected: &str) -> bool {
    let (given, expected) = (given.as_bytes(), expected.as_bytes());
    given.len() == expected.len()
        && given
            .iter()
            .zip(expected)
            .fold(0u8, |diff, (a, b)| diff | (a ^ b))
            == 0
}

/// Root of the workspace, where `target/` is: two levels above this crate
/// (`packages/hurlui_core`). Taken from the ancestors of the manifest
/// directory rather than with `..`, so that the paths derived from it are
/// the ones a user would type.
fn workspace_root() -> &'static Path {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.ancestors().nth(2).unwrap_or(manifest_dir)
}

/// Writes the token where the processes of the current user (automation,
/// tests) can read it, and nobody else: the browser gets it from the URL.
fn write_token(path: &Path, token: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // Never reuse a file created by someone else, or with other permissions.
    let _ = std::fs::remove_file(path);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?.write_all(token.as_bytes())
}

fn header<'a>(request: &'a tiny_http::Request, name: &'static str) -> Option<&'a str> {
    request
        .headers()
        .iter()
        .find(|h| h.field.equiv(name))
        .map(|h| h.value.as_str())
}

fn respond(request: tiny_http::Request, status: u16, body: Value) {
    let content_type =
        tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
    let response = tiny_http::Response::from_string(body.to_string())
        .with_status_code(status)
        .with_header(content_type);
    let _ = request.respond(response);
}

fn main() {
    let port: u16 = std::env::var("HURLUI_BRIDGE_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(1421);
    // Not in the temporary directory: under a fixed name there, another user
    // of the machine could create it first, plant a `state.json` (whose
    // `hurlPath` is a program the core starts) or read `secrets.json`. The
    // core creates the directory when it first writes, for this user only.
    let config_dir = std::env::var_os("HURLUI_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().join("target/dev-bridge-config"));
    let token_file = std::env::var_os("HURLUI_BRIDGE_TOKEN_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().join("target/dev-bridge.token"));
    let origins: Vec<String> = std::env::var("HURLUI_BRIDGE_ORIGINS")
        .unwrap_or_else(|_| DEFAULT_ORIGINS.to_string())
        .split(',')
        .map(|o| o.trim().trim_end_matches('/').to_string())
        .filter(|o| !o.is_empty())
        .collect();

    let token = random_token();
    write_token(&token_file, &token).expect("cannot write the bridge token file");

    let events: Arc<Mutex<Vec<Value>>> = Arc::default();
    let queue = events.clone();
    let ctx = Ctx::new(
        config_dir.clone(),
        Box::new(move |event, payload| {
            queue
                .lock()
                .unwrap()
                .push(json!({ "event": event, "payload": payload }));
        }),
    );

    let server = tiny_http::Server::http(("127.0.0.1", port)).expect("cannot bind the bridge port");
    eprintln!(
        "hurlui dev bridge listening on http://127.0.0.1:{port} (config: {}, token: {})",
        config_dir.display(),
        token_file.display()
    );
    // The fragment is not sent to the Vite server: the token only reaches
    // the page, which removes it from the address bar and keeps it.
    match origins.first() {
        Some(frontend) => {
            eprintln!("Open the frontend at this URL (it carries the access token):");
            eprintln!("  {frontend}/#bridge-token={token}");
        }
        None => eprintln!("No origin is allowed (HURLUI_BRIDGE_ORIGINS): browsers are refused."),
    }
    let token = Arc::new(token);
    let origins = Arc::new(origins);
    for mut request in server.incoming_requests() {
        let ctx = ctx.clone();
        let events = events.clone();
        let token = token.clone();
        let origins = origins.clone();
        // One thread per request: a run may last and must stay cancellable.
        std::thread::spawn(move || {
            if !header(&request, TOKEN_HEADER).is_some_and(|given| same_token(given, &token)) {
                return respond(
                    request,
                    403,
                    json!({ "error": "missing or wrong bridge token" }),
                );
            }
            if header(&request, "Origin").is_some_and(|o| !origins.iter().any(|a| a == o)) {
                return respond(request, 403, json!({ "error": "origin not allowed" }));
            }
            let url = request.url().to_string();
            if url == "/events" {
                let drained: Vec<Value> = std::mem::take(&mut *events.lock().unwrap());
                return respond(request, 200, json!({ "ok": drained }));
            }
            let Some(cmd) = url.strip_prefix("/api/") else {
                return respond(request, 404, json!({ "error": "not found" }));
            };
            let mut body = String::new();
            if request.as_reader().read_to_string(&mut body).is_err() {
                return respond(request, 400, json!({ "error": "unreadable body" }));
            }
            let args: Value = serde_json::from_str(&body).unwrap_or(json!({}));
            let payload = match dispatch(&ctx, cmd, args) {
                Ok(value) => json!({ "ok": value }),
                Err(message) => json!({ "error": message }),
            };
            respond(request, 200, payload);
        });
    }
}
