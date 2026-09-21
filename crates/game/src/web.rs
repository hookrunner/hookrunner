use bevy::{prelude::*, window::CursorOptions};
use hookrunner_client::ServerUrl;

pub fn configure(app: &mut App) {
    let window = web_sys::window().unwrap();
    let location = window.location();
    let params =
        web_sys::UrlSearchParams::new_with_str(&location.search().unwrap_or_default()).unwrap();
    let server = params.get("server").unwrap_or_else(|| {
        let scheme = if location.protocol().unwrap_or_default() == "https:" {
            "wss"
        } else {
            "ws"
        };
        format!("{scheme}://{}:5000", location.hostname().unwrap())
    });
    app.insert_resource(ServerUrl(server)).add_plugins(
        DefaultPlugins
            .set(AssetPlugin {
                file_path: window
                    .document()
                    .unwrap()
                    .get_element_by_id("arena")
                    .unwrap()
                    .get_attribute("data-asset-root")
                    .unwrap(),
                ..default()
            })
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Hookrunner".into(),
                    canvas: Some("#game".into()),
                    fit_canvas_to_parent: true,
                    prevent_default_event_handling: true,
                    ..default()
                }),
                ..default()
            }),
    );
}

pub fn pointer_locked(_cursor: &CursorOptions) -> bool {
    web_sys::window()
        .and_then(|w| w.document())
        .is_some_and(|d| d.pointer_lock_element().is_some())
}

pub fn finished_loading() {
    set_attribute("loading", "style", "display:none");
}

fn set_attribute(id: &str, name: &str, value: &str) {
    if let Some(element) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id(id))
    {
        element.set_attribute(name, value).unwrap();
    }
}
