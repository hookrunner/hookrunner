mod baked_material;
mod loading;
mod map;
mod mipmaps;
mod view;
mod weapon;

#[cfg(not(target_arch = "wasm32"))]
#[path = "native.rs"]
mod platform;
#[cfg(target_arch = "wasm32")]
#[path = "web.rs"]
mod platform;

use bevy::{prelude::*, winit::WinitSettings};
use hookrunner_client::GameNetworkingPlugin;

fn main() -> AppExit {
    let mut app = App::new();
    platform::configure(&mut app);
    app.insert_resource(WinitSettings::continuous())
        .add_plugins((
            GameNetworkingPlugin,
            loading::LoadingPlugin,
            MaterialPlugin::<baked_material::BakedMaterial>::default(),
            view::ViewPlugin,
            weapon::WeaponPlugin,
        ))
        .run()
}
