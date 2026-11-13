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
//! Tauri shell of Hurl UI. All the logic lives in `hurlui_core`; this crate
//! only forwards frontend calls to its dispatcher and events to the webview.

use hurlui_core::Ctx;
use serde_json::Value;
use std::sync::Arc;
use tauri::{Emitter, Manager, State};

/// Single entry point of the frontend: `invoke('api', { cmd, args })`.
/// Commands run on the blocking pool: they touch the disk and wait for `hurl`.
#[tauri::command]
async fn api(ctx: State<'_, Arc<Ctx>>, cmd: String, args: Value) -> Result<Value, String> {
    let ctx = ctx.inner().clone();
    tauri::async_runtime::spawn_blocking(move || hurlui_core::dispatch(&ctx, &cmd, args))
        .await
        .map_err(|e| e.to_string())?
}

/// Buttons of the title bar, on Linux.
///
/// There, the title bar of the window is drawn by GTK, with the buttons of
/// the desktop setting `gtk-decoration-layout`. The default of GNOME, which
/// is also what GTK gets without a desktop session (WSL for instance), has a
/// close button only. Hurl UI is a window one keeps next to others: it always
/// has its minimize and maximize buttons, next to the close button and on its
/// side.
#[cfg(any(target_os = "linux", test))]
mod decoration {
    /// Layout with a minimize and a maximize button, `None` if `layout`
    /// already has them. A layout is a list of buttons on the left, a colon,
    /// and a list of buttons on the right.
    pub fn with_window_buttons(layout: &str) -> Option<String> {
        let (left, right) = layout.split_once(':').unwrap_or((layout, ""));
        let buttons = |side: &str| -> Vec<String> {
            let names = side.split(',').map(str::trim);
            names.filter(|b| !b.is_empty()).map(String::from).collect()
        };
        let (mut left, mut right) = (buttons(left), buttons(right));
        let has = |name: &str| left.iter().chain(&right).any(|b| b == name);
        let missing: Vec<String> = ["minimize", "maximize"]
            .into_iter()
            .filter(|b| !has(b))
            .map(String::from)
            .collect();
        if missing.is_empty() {
            return None;
        }
        // The close button is the outermost one: last on the right, first on
        // the left.
        if let Some(close) = left.iter().position(|b| b == "close") {
            left.splice(close + 1..close + 1, missing);
        } else {
            let close = right.iter().position(|b| b == "close");
            let at = close.unwrap_or(right.len());
            right.splice(at..at, missing);
        }
        Some(format!("{}:{}", left.join(","), right.join(",")))
    }

    /// Called on the main thread, once GTK is initialized.
    #[cfg(target_os = "linux")]
    pub fn show_window_buttons() {
        use gtk::prelude::GtkSettingsExt;

        let Some(settings) = gtk::Settings::default() else {
            return;
        };
        let layout = settings.gtk_decoration_layout().unwrap_or_default();
        if let Some(layout) = with_window_buttons(&layout) {
            settings.set_gtk_decoration_layout(Some(&layout));
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default();
    // A single process owns the configuration directory: every instance
    // rewrites the whole `state.json` from its own memory, so two of them
    // would lose each other's collections. A second launch hands over to the
    // first one, which comes back to the front. Registered before any other
    // plugin, as the plugin requires.
    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // Best effort: a window which cannot be raised is not a reason to
            // bring the running instance down.
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }));
    }
    builder
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            #[cfg(target_os = "linux")]
            decoration::show_window_buttons();
            let config_dir = app.path().app_config_dir()?;
            let handle = app.handle().clone();
            let ctx = Ctx::new(
                config_dir,
                Box::new(move |event, payload| {
                    let _ = handle.emit(event, payload);
                }),
            );
            app.manage(ctx);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![api])
        .run(tauri::generate_context!())
        .expect("error while running Hurl UI");
}

#[cfg(test)]
mod tests {
    use super::decoration::with_window_buttons;

    #[test]
    fn window_buttons_are_added_next_to_the_close_button() {
        let layout = |l: &str| with_window_buttons(l);
        // Default of GNOME, and of GTK without a desktop session.
        assert_eq!(
            layout("appmenu:close").unwrap(),
            "appmenu:minimize,maximize,close"
        );
        assert_eq!(layout("close:").unwrap(), "close,minimize,maximize:");
        assert_eq!(
            layout("close,maximize:menu").unwrap(),
            "close,minimize,maximize:menu"
        );
        assert_eq!(
            layout("menu:maximize,close").unwrap(),
            "menu:maximize,minimize,close"
        );
        assert_eq!(layout("").unwrap(), ":minimize,maximize");
        assert_eq!(layout("menu").unwrap(), "menu:minimize,maximize");
        // Nothing to do: the choice of the desktop is kept as it is.
        assert_eq!(layout("menu:minimize,maximize,close"), None);
        assert_eq!(layout("close,minimize,maximize:"), None);
        assert_eq!(layout("minimize:maximize"), None);
    }
}
