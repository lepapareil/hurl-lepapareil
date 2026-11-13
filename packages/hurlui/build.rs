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
//! Build script of Hurl UI.
//!
//! When the frontend is embedded in the application (`custom-protocol`
//! feature, enabled by default), it is built here, by `npm`, into `dist`:
//! `cargo build` is the only command needed to build the application, and
//! Node.js is the only tool needed besides Rust. Without this feature, the
//! application loads the frontend from the Vite development server and
//! nothing is built here.

use std::path::Path;
use std::process::Command;
use std::time::SystemTime;

/// Sources of the frontend: the build script is run again when they change.
const FRONTEND: &[&str] = &[
    "ui",
    // Logo of Hurl, used by the frontend.
    "../../art/logo-icon.svg",
    "package.json",
    "package-lock.json",
    "tsconfig.json",
    "vite.config.ts",
];

fn npm(args: &[&str]) {
    // On Windows, npm is a batch file: the extension has to be given.
    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
    let command = format!("{npm} {}", args.join(" "));
    let status = Command::new(npm).args(args).status().unwrap_or_else(|e| {
        panic!(
            "cannot run `{command}`: {e}. Node.js (https://nodejs.org) is needed to build the \
             frontend of Hurl UI."
        )
    });
    assert!(status.success(), "`{command}` failed: {status}");
}

fn modified(path: &str) -> Option<SystemTime> {
    Path::new(path).metadata().ok()?.modified().ok()
}

/// Whether the installed packages are the ones of the lock file. npm writes
/// `node_modules/.package-lock.json` at the end of every installation.
fn packages_are_installed() -> bool {
    match (
        modified("node_modules/.package-lock.json"),
        modified("package-lock.json"),
    ) {
        (Some(installed), Some(lock)) => installed >= lock,
        _ => false,
    }
}

fn build_frontend() {
    for path in FRONTEND {
        println!("cargo:rerun-if-changed={path}");
    }
    if !packages_are_installed() {
        npm(&["ci", "--no-audit", "--no-fund"]);
    }
    npm(&["run", "build"]);
}

fn main() {
    if std::env::var_os("CARGO_FEATURE_CUSTOM_PROTOCOL").is_some() {
        build_frontend();
    }
    tauri_build::build();
}
