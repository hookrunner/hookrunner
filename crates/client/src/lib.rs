use bevy::prelude::*;
use hookrunner_shared::{
    PlayerId, PlayerInput, PlayerState, ProtocolPlugin, TICK_DURATION, movement,
};
use lightyear::interpolation::timeline::InterpolationConfig;
use lightyear::prelude::{client::*, input::native::*, *};

#[derive(Resource, Clone)]
pub struct ServerUrl(pub String);

pub struct GameNetworkingPlugin;

#[derive(Resource, Default)]
pub struct NetworkStats {
    pub frame_ms: f32,
    /// Network round-trip time; absent while disconnected.
    pub ping_ms: Option<f32>,
}

impl Plugin for GameNetworkingPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(ClientPlugins {
            tick_duration: TICK_DURATION,
        })
        .add_plugins(ProtocolPlugin)
        .init_resource::<NetworkStats>()
        .add_systems(Startup, connect)
        .add_systems(PreUpdate, attach_input)
        .add_systems(FixedUpdate, predict)
        .add_systems(Update, update_network_stats);
    }
}

fn connect(mut commands: Commands, url: Res<ServerUrl>) {
    let mut endpoint = url::Url::parse(&url.0).expect("game server URL must be valid");
    endpoint
        .query_pairs_mut()
        .append_pair("build", hookrunner_shared::SIMULATION_BUILD);
    #[cfg(target_arch = "wasm32")]
    let config = ClientConfig;
    #[cfg(not(target_arch = "wasm32"))]
    let config = ClientConfig::builder().with_native_certs().disable_nagle();
    let entity = commands
        .spawn((
            RawClient,
            // Browsers do not expose their local socket address. Ownership is assigned by the server.
            LocalAddr("0.0.0.0:0".parse().unwrap()),
            ReplicationReceiver::default(),
            PredictionManager::default(),
            InterpolationConfig {
                // Buffer one and a half 120 Hz snapshots (~12.5 ms), plus the
                // clock synchronizer's adaptive jitter allowance.
                send_interval_ratio: 1.5,
                sync: SyncConfig {
                    jitter_margin: std::time::Duration::from_millis(1),
                    error_margin: 0.25,
                    ..default()
                },
                ..default()
            },
            InputTimelineConfig::default().with_sync_config(SyncConfig {
                // Inputs must arrive before the server enters their fixed tick. A sub-tick
                // margin can leave every packet one tick late even on loopback.
                jitter_margin: TICK_DURATION * 2,
                error_margin: 0.5,
                ..default()
            }),
            WebSocketClientIo::from_url(config, endpoint.as_str()),
        ))
        .id();
    commands.trigger(Connect { entity });
}

type LocalPlayerWithoutInput = (
    With<PlayerId>,
    With<Predicted>,
    Without<InputMarker<PlayerInput>>,
);

fn attach_input(mut commands: Commands, players: Query<Entity, LocalPlayerWithoutInput>) {
    for entity in &players {
        commands
            .entity(entity)
            .insert(InputMarker::<PlayerInput>::default());
    }
}

fn predict(mut players: Query<(&mut PlayerState, &ActionState<PlayerInput>), With<Predicted>>) {
    for (mut state, input) in &mut players {
        movement::step(&mut state, &input.0);
    }
}

fn update_network_stats(
    time: Res<Time<Real>>,
    connections: Query<&PingManager, With<Connected>>,
    mut stats: ResMut<NetworkStats>,
) {
    let dt = time.delta_secs();
    if dt > 0.0 {
        let frame_ms = dt * 1000.0;
        let weight = 1.0 - (-dt / 0.5).exp();
        stats.frame_ms = if stats.frame_ms == 0.0 {
            frame_ms
        } else {
            stats.frame_ms + (frame_ms - stats.frame_ms) * weight
        };
    }
    stats.ping_ms = connections
        .iter()
        .next()
        .map(|ping| ping.rtt().as_secs_f32() * 1000.0);
}
