//! WebKitGTK denies getUserMedia unless the embedder handles its permission request.

use tauri::{Manager, Url};
use webkit2gtk::{
    glib::prelude::Cast, PermissionRequestExt, SettingsExt, UserMediaPermissionRequest,
    UserMediaPermissionRequestExt, WebViewExt,
};

pub fn configure(app: &tauri::App) -> tauri::Result<()> {
    let Some(window) = app.get_webview_window("main") else {
        return Ok(());
    };
    let app_url = if tauri::is_dev() {
        app.config().build.dev_url.clone()
    } else {
        None
    }
    .unwrap_or_else(|| Url::parse("tauri://localhost").expect("valid app URL"));

    window.with_webview(move |platform| {
        let webview = platform.inner();
        if let Some(settings) = webview.settings() {
            settings.set_enable_media_stream(true);
        }
        webview.connect_permission_request(move |view, request| {
            let Some(media) = request.downcast_ref::<UserMediaPermissionRequest>() else {
                return false;
            };
            let trusted_page = view
                .uri()
                .and_then(|uri| Url::parse(&uri).ok())
                .is_some_and(|url| same_origin(&url, &app_url));
            if trusted_page && media.is_for_audio_device() && !media.is_for_video_device() {
                request.allow();
            } else {
                request.deny();
            }
            true
        });
    })
}

fn same_origin(url: &Url, expected: &Url) -> bool {
    url.scheme() == expected.scheme()
        && url.host_str() == expected.host_str()
        && url.port_or_known_default() == expected.port_or_known_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn microphone_access_stays_on_the_app_origin() {
        for (app, allowed, denied) in [
            (
                "tauri://localhost",
                "tauri://localhost/chat/123",
                "https://localhost/chat/123",
            ),
            (
                "http://localhost:1420",
                "http://localhost:1420/chat/123",
                "http://localhost:1421/chat/123",
            ),
        ] {
            let app = Url::parse(app).unwrap();
            assert!(same_origin(&Url::parse(allowed).unwrap(), &app));
            assert!(!same_origin(&Url::parse(denied).unwrap(), &app));
            assert!(!same_origin(
                &Url::parse("https://example.com").unwrap(),
                &app
            ));
        }
    }
}
