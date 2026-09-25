mod baked_material;
mod loading;
mod map;
mod mipmaps;
mod scoreboard;
mod title;
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
    // Bundle the full font for identical Unicode coverage on web and native.
    app.world_mut()
        .resource_mut::<Assets<Font>>()
        .insert(
            bevy::asset::AssetId::default(),
            Font::try_from_bytes(
                include_bytes!("../../../assets/fonts/DejaVuSansMono.ttf").to_vec(),
            )
            .expect("bundled UI font must be valid"),
        )
        .unwrap();
    app.insert_resource(WinitSettings::continuous())
        .add_plugins((
            GameNetworkingPlugin,
            loading::LoadingPlugin,
            title::TitlePlugin,
            scoreboard::ScoreboardPlugin,
            MaterialPlugin::<baked_material::BakedMaterial>::default(),
            view::ViewPlugin,
            weapon::WeaponPlugin,
        ))
        .run()
}
