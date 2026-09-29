use bevy::prelude::*;
use lightyear::prelude::server::Started;
use std::time::Instant;

pub struct LoadingPlugin;

impl Plugin for LoadingPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(ready);
    }
}

#[derive(Resource)]
struct LoadingStarted(Instant);

pub fn prepare_world(commands: &mut Commands) {
    let started = Instant::now();
    info!("Loading game [0/4, 0%]: reading map metadata");
    let map = hookrunner_shared::level::data();
    info!(
        "Loading game [1/4, 25%]: {} loaded ({} spawns, {} triggers)",
        map.name,
        map.spawns.len(),
        map.triggers.len()
    );
    info!("Loading game [1/4, 25%]: building collision world");
    let _ = hookrunner_shared::level::world();
    info!("Loading game [2/4, 50%]: collision world ready");
    commands.insert_resource(LoadingStarted(started));
}

// The transport confirms readiness asynchronously after binding its socket.
fn ready(_event: On<Add, Started>, started: Res<LoadingStarted>) {
    info!(
        "Loading game [4/4, 100%]: ready for players ({:.2}s)",
        started.0.elapsed().as_secs_f64()
    );
}
