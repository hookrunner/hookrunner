mod build_check;
mod combat;
mod history;
mod loading;
mod matches;

use bevy::{app::ScheduleRunnerPlugin, prelude::*};
use hookrunner_shared::{
    PlayerId, PlayerInput, PlayerState, ProtocolPlugin, SEND_INTERVAL, TICK_DURATION, TICK_HZ,
    arena, movement,
    player_color::{PALETTE, PlayerColor},
    protocol::{JoinRejected, JoinRequest, LobbyChannel, PlayerName},
    weapon::Projectile,
};
use lightyear::prelude::{input::native::ActionState, server::*, *};
use std::{net::SocketAddr, time::Duration};

fn main() {
    if std::env::args().nth(1).as_deref() == Some("--print-build") {
        println!("{}", hookrunner_shared::SIMULATION_BUILD);
        return;
    }
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
        .add_plugins(loading::LoadingPlugin)
        .insert_resource(BindAddress(address))
        .init_resource::<hookrunner_shared::match_state::MatchState>()
        .init_resource::<matches::MatchClock>()
        .init_resource::<history::PlayerHistory>()
        .add_systems(Startup, (start, matches::setup))
        .add_systems(PreUpdate, matches::advance)
        .add_observer(configure_link)
        .add_systems(Update, spawn_players)
        .add_observer(log_disconnect)
        .add_systems(
            FixedUpdate,
            (
                history::capture_before,
                simulate,
                history::capture_after,
                combat::advance_projectiles,
            )
                .chain(),
        )
        .add_systems(
            PostUpdate,
            (combat::publish_projectiles, matches::publish).before(ReplicationSystems::Send),
        )
        .run();
}

#[derive(Resource)]
struct BindAddress(SocketAddr);

fn start(mut commands: Commands, address: Res<BindAddress>) {
    loading::prepare_world(&mut commands);
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
    info!(
        "Loading game [3/4, 75%]: opening WebSocket listener at {}",
        address.0
    );
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

#[derive(Component)]
struct Joined;

fn spawn_players(
    mut links: Query<
        (
            Entity,
            &RemoteId,
            &mut MessageReceiver<JoinRequest>,
            &mut MessageSender<JoinRejected>,
            Has<Joined>,
        ),
        (With<ClientOf>, With<Connected>),
    >,
    players: Query<(&PlayerState, &PlayerColor)>,
    round: Res<hookrunner_shared::match_state::MatchState>,
    mut commands: Commands,
) {
    let mut occupied = Vec::new();
    let mut occupied_colors = [false; PALETTE.len()];
    for (state, color) in &players {
        occupied.push(state.position);
        occupied_colors[color.0 as usize] = true;
    }
    for (entity, remote, mut requests, mut replies, already_joined) in &mut links {
        let mut joined = already_joined;
        for request in requests.receive() {
            if joined {
                continue;
            }
            let name = match hookrunner_shared::nickname::validate(&request.nickname) {
                Ok(name) => name,
                Err(reason) => {
                    replies.send::<LobbyChannel>(JoinRejected {
                        reason: reason.into(),
                    });
                    continue;
                }
            };
            let Some(color) = PlayerColor::new(request.color) else {
                replies.send::<LobbyChannel>(JoinRejected {
                    reason: "Choose a valid color.".into(),
                });
                continue;
            };
            if occupied_colors[color.0 as usize] {
                replies.send::<LobbyChannel>(JoinRejected {
                    reason: "This color is taken. Choose another.".into(),
                });
                continue;
            }
            if occupied.len() >= arena::MAX_PLAYERS {
                replies.send::<LobbyChannel>(JoinRejected {
                    reason: "The server is full. Please try again later.".into(),
                });
                continue;
            }
            let spawn_index = arena::choose_spawn(rand::random::<u32>() as usize, &occupied);
            let spawn = arena::spawn(spawn_index);
            let position = spawn.position;
            occupied.push(position);
            occupied_colors[color.0 as usize] = true;
            let id = rand::random::<u64>();
            commands.spawn((
                PlayerId(id),
                PlayerName(name.clone()),
                color,
                PlayerState {
                    position,
                    yaw: spawn.yaw,
                    view_yaw_offset: spawn.yaw,
                    spawn_index: spawn_index as u8,
                    grounded: true,
                    match_paused: round.results,
                    ..default()
                },
                Replicate::to_clients(NetworkTarget::All),
                PredictionTarget::to_clients(NetworkTarget::Single(remote.0)),
                InterpolationTarget::to_clients(NetworkTarget::AllExceptSingle(remote.0)),
                ControlledBy {
                    owner: entity,
                    lifetime: Default::default(),
                },
            ));
            commands.entity(entity).insert(Joined);
            joined = true;
            info!(
                "Player {id:016x} ({name}) joined at {position} ({:?})",
                remote.0
            );
        }
    }
}

fn simulate(
    mut commands: Commands,
    mut round: ResMut<hookrunner_shared::match_state::MatchState>,
    mut players: Query<(
        &PlayerId,
        &mut PlayerState,
        &ActionState<PlayerInput>,
        &ControlledBy,
    )>,
    links: Query<(
        &PingManager,
        Option<&lightyear::interpolation::plugin::InterpolationDelay>,
    )>,
) {
    for (id, mut state, input, control) in &mut players {
        let was_alive = state.death.is_none();
        let previous_shot = state.weapon.shot;
        movement::step(&mut state, &input.0);
        if was_alive && state.death.is_some() {
            round.record_death(id.0, None);
        }
        if state.weapon.shot != previous_shot {
            let lag = links
                .get(control.owner)
                .ok()
                .map_or(0.0, |(ping, delay)| history::validated_delay(ping, delay));
            commands.spawn((
                Projectile::from_shot(id.0, &state, &input.0),
                history::ProjectileLag(lag),
            ));
        }
    }
}

fn log_disconnect(event: On<Add, Disconnected>, links: Query<&RemoteId, With<ClientOf>>) {
    if let Ok(remote) = links.get(event.entity) {
        info!("Player disconnected: {:?}", remote.0);
    }
}
