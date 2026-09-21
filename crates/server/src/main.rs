mod build_check;
mod combat;

use bevy::{app::ScheduleRunnerPlugin, prelude::*};
use hookrunner_shared::{
    PlayerId, PlayerInput, PlayerState, ProtocolPlugin, SEND_INTERVAL, TICK_DURATION, TICK_HZ,
    arena, movement, weapon::Projectile,
};
use lightyear::prelude::{input::native::ActionState, server::*, *};
use std::{net::SocketAddr, time::Duration};

fn main() {
    let address: SocketAddr = std::env::var("HOOKRUNNER_BIND")
        .unwrap_or_else(|_| "0.0.0.0:5000".into())
        .parse()
        .expect("HOOKRUNNER_BIND must be an IP:port");
    App::new()
        .add_plugins((
            MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(2))),
            bevy::log::LogPlugin::default(),
        ))
        .add_plugins(ServerPlugins {
            tick_duration: TICK_DURATION,
        })
        .add_plugins(ProtocolPlugin)
        .insert_resource(BindAddress(address))
        .add_systems(Startup, start)
        .add_observer(configure_link)
        .add_observer(spawn_player)
        .add_observer(log_disconnect)
        .add_systems(FixedUpdate, (simulate, combat::advance_projectiles).chain())
        .add_systems(
            PostUpdate,
            combat::publish_projectiles.before(ReplicationSystems::Send),
        )
        .run();
}

#[derive(Resource)]
struct BindAddress(SocketAddr);

fn start(mut commands: Commands, address: Res<BindAddress>) {
    let _ = hookrunner_shared::level::world();
    let config = lightyear::websocket::server::ServerConfig::builder()
        .with_bind_address(address.0)
        .with_no_encryption()
        .with_handshake_handler(build_check::handshake());
    let entity = commands
        .spawn((
            RawServer,
            LocalAddr(address.0),
            WebSocketServerIo { config },
        ))
        .id();
    commands.trigger(Start { entity });
    info!(
        "Hookrunner: ws://{} ({TICK_HZ:.0} Hz simulation, {TICK_HZ:.0} Hz snapshots)",
        address.0
    );
    info!("Simulation build: {}", hookrunner_shared::SIMULATION_BUILD);
}

fn configure_link(event: On<Add, LinkOf>, mut commands: Commands) {
    commands.entity(event.entity).insert(ReplicationSender::new(
        SEND_INTERVAL,
        SendUpdatesMode::SinceLastAck,
        false,
    ));
}

fn spawn_player(
    event: On<Add, Connected>,
    links: Query<&RemoteId, With<ClientOf>>,
    players: Query<&PlayerState>,
    mut commands: Commands,
) {
    let Ok(remote) = links.get(event.entity) else {
        return;
    };
    let occupied: Vec<_> = players.iter().map(|p| p.position).collect();
    if occupied.len() >= arena::MAX_PLAYERS {
        commands.trigger(Disconnect {
            entity: event.entity,
        });
        return;
    }
    let spawn_index = arena::choose_spawn(rand::random::<u32>() as usize, &occupied);
    let spawn = arena::spawn(spawn_index);
    let position = spawn.position;
    let id = rand::random::<u64>();
    commands.spawn((
        PlayerId(id),
        PlayerState {
            position,
            yaw: spawn.yaw,
            view_yaw_offset: spawn.yaw,
            spawn_index: spawn_index as u8,
            grounded: true,
            ..default()
        },
        Replicate::to_clients(NetworkTarget::All),
        PredictionTarget::to_clients(NetworkTarget::Single(remote.0)),
        InterpolationTarget::to_clients(NetworkTarget::AllExceptSingle(remote.0)),
        ControlledBy {
            owner: event.entity,
            lifetime: Default::default(),
        },
    ));
    info!("Player {id:016x} joined at {position} ({:?})", remote.0);
}

fn simulate(
    mut commands: Commands,
    mut players: Query<(&PlayerId, &mut PlayerState, &ActionState<PlayerInput>)>,
) {
    for (id, mut state, input) in &mut players {
        let previous_shot = state.weapon.shot;
        movement::step(&mut state, &input.0);
        if state.weapon.shot != previous_shot {
            commands.spawn(Projectile::from_shot(id.0, &state, &input.0));
        }
    }
}

fn log_disconnect(event: On<Add, Disconnected>, links: Query<&RemoteId, With<ClientOf>>) {
    if let Ok(remote) = links.get(event.entity) {
        info!("Player disconnected: {:?}", remote.0);
    }
}
