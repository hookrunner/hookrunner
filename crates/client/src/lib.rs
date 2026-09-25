use bevy::prelude::*;
use hookrunner_shared::{
    PlayerId, PlayerInput, PlayerState, ProtocolPlugin, TICK_DURATION, movement,
    protocol::{JoinRejected, JoinRequest, LobbyChannel, PlayerName},
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
        .init_resource::<Session>()
        .add_observer(connect)
        .add_systems(Update, (send_join, update_session).chain())
        .add_systems(PreUpdate, attach_input)
        .add_systems(FixedUpdate, predict)
        .add_systems(Update, update_network_stats);
    }
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum SessionPhase {
    #[default]
    Title,
    Connecting,
    Playing,
}

#[derive(Resource, Default)]
pub struct Session {
    pub phase: SessionPhase,
    pub nickname: String,
    pub error: Option<String>,
    connection: Option<Entity>,
    elapsed: f32,
}

impl Session {
    pub fn is_playing(&self) -> bool {
        self.phase == SessionPhase::Playing
    }
}

#[derive(Event)]
pub struct JoinGame {
    pub nickname: String,
}

fn connect(
    event: On<JoinGame>,
    mut commands: Commands,
    url: Res<ServerUrl>,
    mut session: ResMut<Session>,
) {
    if session.phase != SessionPhase::Title {
        return;
    }
    let nickname = match hookrunner_shared::nickname::validate(&event.nickname) {
        Ok(nickname) => nickname,
        Err(error) => {
            session.error = Some(error.into());
            return;
        }
    };
    session.nickname = nickname;
    session.error = None;
    session.elapsed = 0.0;
    session.phase = SessionPhase::Connecting;
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
    commands.entity(entity).insert(Connecting);
    session.connection = Some(entity);
    commands.trigger(Connect { entity });
}

fn send_join(
    session: Res<Session>,
    mut connections: Query<&mut MessageSender<JoinRequest>, (With<Client>, Added<Connected>)>,
) {
    for mut sender in &mut connections {
        sender.send::<LobbyChannel>(JoinRequest {
            nickname: session.nickname.clone(),
        });
    }
}

pub fn update_session(
    mut commands: Commands,
    time: Res<Time<Real>>,
    mut session: ResMut<Session>,
    mut connections: Query<
        (Option<&Disconnected>, &mut MessageReceiver<JoinRejected>),
        With<Client>,
    >,
    players: Query<&PlayerName, With<Predicted>>,
    replicas: Query<Entity, Or<(With<PlayerId>, With<hookrunner_shared::weapon::Projectile>)>>,
) {
    let Some(entity) = session.connection else {
        return;
    };
    let mut failure = None;
    if let Ok((disconnected, mut replies)) = connections.get_mut(entity) {
        for reply in replies.receive() {
            failure = Some(reply.reason);
        }
        if disconnected.is_some_and(|state| state.reason.is_some() || session.is_playing())
            && failure.is_none()
        {
            failure = Some("Disconnected. Check the server and try again.".into());
        }
    } else {
        failure = Some("Connection lost. Please try again.".into());
    }
    if session.phase == SessionPhase::Connecting {
        session.elapsed += time.delta_secs();
        if let Some(name) = players.iter().next() {
            session.nickname = name.0.clone();
            session.phase = SessionPhase::Playing;
        } else if session.elapsed >= 15.0 && failure.is_none() {
            failure = Some("Connection timed out. Check the server and try again.".into());
        }
    }
    if let Some(error) = failure {
        commands.trigger(Disconnect { entity });
        commands.entity(entity).try_despawn();
        for replica in &replicas {
            commands.entity(replica).try_despawn();
        }
        session.connection = None;
        session.phase = SessionPhase::Title;
        session.error = Some(error);
    }
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
