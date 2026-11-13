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
//! Core of Hurl UI, independent from Tauri so that it can be unit tested and
//! driven either by the desktop shell (`packages/hurlui`) or by the development
//! bridge (`examples/dev_bridge.rs`).

pub mod api;
pub mod collection;
pub mod hurlfile;
pub mod options;
pub mod runner;
pub mod store;
pub mod watcher;

pub use api::{Ctx, dispatch};
