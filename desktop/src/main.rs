#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod backend;

use backend::{ApiConnection, Backend};
use numera_cha_solver::{
    config::{Settings, runtime_library_name},
    logging,
};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};
use tauri::{Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};

fn trusted_url(url: &tauri::Url) -> bool {
    url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && matches!(
            (url.scheme(), url.host_str()),
            ("tauri", Some("localhost")) | ("http", Some("tauri.localhost"))
        )
}

#[tauri::command]
fn api_connection(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Backend>,
) -> Result<ApiConnection, &'static str> {
    if window.label() != "main" || !window.url().is_ok_and(|url| trusted_url(&url)) {
        return Err("Only the embedded workspace may request its local connection.");
    }
    Ok(state.connection.clone())
}

fn main() {
    let result = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![api_connection])
        .setup(|app| {
            let mut settings = Settings::from_env()?;
            if std::env::var_os("NUMERA_ROOT").is_none() {
                settings.root = app.path().app_local_data_dir()?;
            }
            if std::env::var_os("NUMERA_DEVICE").is_none()
                && std::env::var_os("NUMERA_ORT_LIBRARY").is_none()
            {
                settings.device = "cpu".into();
            }
            if std::env::var_os("NUMERA_ORT_LIBRARY").is_none() {
                settings.ort_library = app
                    .path()
                    .resource_dir()?
                    .join("data/runtime")
                    .join(runtime_library_name());
            }
            settings.desktop_origin = Some(if cfg!(windows) {
                "http://tauri.localhost".into()
            } else {
                "tauri://localhost".into()
            });
            let backend = Backend::start(settings)?;
            app.manage(backend);
            WebviewWindowBuilder::new(app, "main", WebviewUrl::App(PathBuf::from("index.html")))
                .title("Numera / Perception Lab")
                .inner_size(1440.0, 1000.0)
                .min_inner_size(375.0, 600.0)
                .on_navigation(trusted_url)
                .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
                .build()?;
            Ok(())
        })
        .build(tauri::generate_context!());
    let app = match result {
        Ok(app) => app,
        Err(_) => {
            logging::event(
                "ERROR",
                "desktop",
                "Desktop startup failed; inspect local logs and prerequisites.",
            );
            std::process::exit(1);
        }
    };
    let stopping = AtomicBool::new(false);
    app.run(move |handle, event| {
        if let RunEvent::ExitRequested { api, .. } = event
            && !stopping.swap(true, Ordering::SeqCst)
        {
            api.prevent_exit();
            let clean = handle.state::<Backend>().stop();
            handle.exit(if clean { 0 } else { 1 });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_embedded_origins_can_read_the_connection() {
        for url in [
            "tauri://localhost/index.html",
            "http://tauri.localhost/#workspace",
        ] {
            assert!(trusted_url(&tauri::Url::parse(url).unwrap()));
        }
        for url in [
            "http://127.0.0.1:1583",
            "https://tauri.localhost",
            "tauri://evil.example",
            "http://tauri.localhost:8080",
            "http://user@tauri.localhost",
        ] {
            assert!(!trusted_url(&tauri::Url::parse(url).unwrap()), "{url}");
        }
    }

    #[test]
    fn owned_backend_readiness_and_close_are_bounded() {
        let root = tempfile::tempdir().unwrap();
        let mut settings = Settings::new(root.path().to_owned());
        settings.device = "cpu".into();
        let backend = Backend::start(settings).unwrap();
        let address = backend.connection.url.strip_prefix("http://").unwrap();
        assert!(!backend.connection.token.is_empty());
        assert!(backend.stop());
        assert!(backend.stop());
        assert!(
            std::net::TcpStream::connect_timeout(
                &address.parse().unwrap(),
                std::time::Duration::from_secs(1)
            )
            .is_err()
        );
    }
}
