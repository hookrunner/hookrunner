use bevy::{
    camera::CameraOutputMode,
    core_pipeline::tonemapping::Tonemapping,
    input::mouse::AccumulatedMouseMotion,
    prelude::*,
    window::{CursorOptions, PrimaryWindow},
};
use hookrunner_client::{NetworkPresentation, NetworkStats, PresentationPosition, Session};
use hookrunner_shared::{
    PlayerId, PlayerInput, PlayerState, arena, level, player_color::PlayerColor,
};
use lightyear::prelude::{client::input::InputSystems, input::native::*, *};

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct CameraUpdated;

pub struct ViewPlugin;

impl Plugin for ViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Look>()
            .insert_resource(ClearColor(Color::srgb(0.055, 0.07, 0.095)))
            .add_systems(Startup, build_scene)
            .add_systems(Startup, crate::map::build_map)
            .add_systems(
                Update,
                (crate::map::finish_loading, crate::map::tune_lighting),
            )
            .add_systems(
                PreUpdate,
                look_input
                    .after(bevy::input::InputSystems)
                    .after(crate::debug_menu::MenuInput),
            )
            .add_systems(
                FixedPreUpdate,
                buffer_input.in_set(InputSystems::WriteClientInputs),
            )
            .add_systems(
                Update,
                (
                    attach_visuals,
                    sync_bodies,
                    sync_shapes,
                    follow_camera.in_set(CameraUpdated),
                    update_hud,
                )
                    .chain()
                    .after(InterpolationSystems::Interpolate)
                    .after(NetworkPresentation),
            );
    }
}

#[derive(Resource, Default)]
pub(crate) struct Look {
    yaw: f32,
    pitch: f32,
    locked: bool,
    dash_press: u16,
    jump_press: u16,
}
#[derive(Component)]
struct PlayerVisual;
#[derive(Component)]
struct BodyShape {
    radius: f32,
    height: f32,
}
#[derive(Component)]
struct Visor;
#[derive(Component)]
pub(crate) struct PlayerCamera;
#[derive(Component)]
struct Hud;
#[derive(Component)]
pub(crate) struct Crosshair;

fn build_scene(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        // The weapon/UI camera presents the combined intermediate color target.
        // Presenting the world first would add a redundant full-resolution blit.
        Camera {
            output_mode: CameraOutputMode::Skip,
            ..default()
        },
        bevy::camera::Exposure::INDOOR,
        // The original lightmap pipeline already produces display-ready lighting.
        Tonemapping::None,
        Projection::Perspective(PerspectiveProjection {
            fov: 90.0_f32.to_radians(),
            ..default()
        }),
        Transform::from_translation(
            arena::spawn(0, default()).position
                + Vec3::Y * hookrunner_shared::tuning::SimulationTuning::default().eye_height,
        )
        .with_rotation(Quat::from_rotation_y(arena::spawn(0, default()).yaw)),
        PlayerCamera,
        Msaa::Sample4,
    ));
    commands.spawn((
        Text::new("0 fps, — ms"),
        TextFont {
            font_size: 18.0,
            ..default()
        },
        TextColor(Color::srgb(0.88, 0.95, 0.96)),
        Node {
            position_type: PositionType::Absolute,
            left: px(22),
            top: px(20),
            ..default()
        },
        Hud,
    ));
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: percent(50),
            top: percent(50),
            width: px(4),
            height: px(4),
            ..default()
        },
        BackgroundColor(Color::WHITE),
        Crosshair,
    ));
}

type PlayerWithoutVisual = (With<PlayerId>, Without<PlayerVisual>);

fn attach_visuals(
    mut commands: Commands,
    players: Query<(Entity, Has<Predicted>, &PlayerColor, &PlayerState), PlayerWithoutVisual>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (entity, local, player_color, state) in &players {
        commands.entity(entity).insert((
            PlayerVisual,
            Transform::default(),
            if local {
                Visibility::Hidden
            } else {
                Visibility::Inherited
            },
        ));
        let [r, g, b] = player_color.rgb();
        let color = materials.add(StandardMaterial {
            base_color: Color::srgb(r, g, b),
            perceptual_roughness: 1.0,
            reflectance: 0.0,
            ..default()
        });
        let visor = materials.add(StandardMaterial {
            base_color: Color::srgb(0.03, 0.05, 0.07),
            unlit: true,
            ..default()
        });
        commands.entity(entity).with_children(|parent| {
            parent.spawn((
                Mesh3d(meshes.add(Capsule3d::new(
                    state.tuning.player_radius,
                    state.tuning.player_height - 2.0 * state.tuning.player_radius,
                ))),
                BodyShape {
                    radius: state.tuning.player_radius,
                    height: state.tuning.player_height,
                },
                MeshMaterial3d(color),
                Transform::from_xyz(0.0, state.tuning.player_height / 2.0, 0.0),
            ));
            parent.spawn((
                Mesh3d(meshes.add(Cuboid::new(0.55, 0.18, 0.16))),
                Visor,
                MeshMaterial3d(visor),
                Transform::from_xyz(0.0, 1.45, -0.34),
            ));
        });
    }
}

pub(crate) fn look_input(
    mut look: ResMut<Look>,
    settings: Res<hookrunner_client::tuning::TuningClient>,
    menu: Res<crate::debug_menu::DebugMenu>,
    session: Res<Session>,
    motion: Res<AccumulatedMouseMotion>,
    keys: Res<ButtonInput<KeyCode>>,
    window: Single<(&Window, &CursorOptions), With<PrimaryWindow>>,
    local: Query<&PlayerState, With<Predicted>>,
) {
    look.locked = session.is_playing()
        && !menu.open
        && !local.iter().any(|p| p.match_paused)
        && window.0.focused
        && crate::platform::pointer_locked(window.1);
    if look.locked {
        if keys.just_pressed(KeyCode::Space) {
            look.jump_press = look.jump_press.wrapping_add(1);
        }
        if keys.just_pressed(KeyCode::ShiftLeft) || keys.just_pressed(KeyCode::ShiftRight) {
            look.dash_press = look.dash_press.wrapping_add(1);
        }
        if !local.iter().any(|state| state.death.is_some()) {
            look.yaw = (look.yaw
                - motion.delta.x * settings.values().presentation.mouse_sensitivity)
                .rem_euclid(std::f32::consts::TAU);
            look.pitch = (look.pitch
                - motion.delta.y * settings.values().presentation.mouse_sensitivity)
                .clamp(-PlayerInput::MAX_PITCH, PlayerInput::MAX_PITCH);
        }
    }
}

fn buffer_input(
    look: Res<Look>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut players: Query<&mut ActionState<PlayerInput>, With<InputMarker<PlayerInput>>>,
) {
    for mut action in &mut players {
        action.0 = PlayerInput {
            forward: look.locked && keys.pressed(KeyCode::KeyW),
            backward: look.locked && keys.pressed(KeyCode::KeyS),
            left: look.locked && keys.pressed(KeyCode::KeyA),
            right: look.locked && keys.pressed(KeyCode::KeyD),
            jump_press: look.jump_press,
            dash_press: look.dash_press,
            fire: look.locked && mouse.pressed(MouseButton::Left),
            yaw: PlayerInput::encode_yaw(look.yaw),
            pitch: PlayerInput::encode_pitch(look.pitch),
        };
    }
}

fn sync_bodies(mut players: Query<(&PlayerState, &mut Transform), With<PlayerVisual>>) {
    for (state, mut transform) in &mut players {
        transform.translation = state.position;
        transform.rotation = Quat::from_rotation_y(state.yaw);
    }
}

fn sync_shapes(
    players: Query<&PlayerState>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut bodies: Query<(&ChildOf, &mut BodyShape, &mut Mesh3d, &mut Transform)>,
    mut visors: Query<(&ChildOf, &mut Transform), (With<Visor>, Without<BodyShape>)>,
) {
    for (parent, mut shape, mut mesh, mut transform) in &mut bodies {
        let Ok(state) = players.get(parent.parent()) else {
            continue;
        };
        let radius = state.tuning.player_radius;
        let height = state.tuning.player_height;
        if shape.radius != radius || shape.height != height {
            mesh.0 = meshes.add(Capsule3d::new(radius, height - 2.0 * radius));
            transform.translation.y = height / 2.0;
            shape.radius = radius;
            shape.height = height;
        }
    }
    for (parent, mut transform) in &mut visors {
        let Ok(state) = players.get(parent.parent()) else {
            continue;
        };
        transform.scale = Vec3::new(
            state.tuning.player_radius / 0.4,
            state.tuning.player_height / 1.8,
            state.tuning.player_radius / 0.4,
        );
        transform.translation = Vec3::new(0.0, 1.45 * transform.scale.y, -0.34 * transform.scale.z);
    }
}

fn follow_camera(
    settings: Res<hookrunner_client::tuning::TuningClient>,
    look: Res<Look>,
    menu: Res<crate::debug_menu::DebugMenu>,
    mut local: Query<
        (&PlayerState, Option<&PresentationPosition>, &mut Visibility),
        With<Predicted>,
    >,
    mut camera: Single<
        (&mut Transform, &mut Projection),
        (With<PlayerCamera>, Without<PlayerState>),
    >,
    mut crosshair: Single<&mut Node, With<Crosshair>>,
) {
    let tuning = settings.values().presentation;
    let (camera, projection) = &mut *camera;
    if let Projection::Perspective(projection) = &mut **projection {
        projection.fov = tuning.camera_fov.to_radians();
    }
    crosshair.width = px(tuning.crosshair_size);
    crosshair.height = px(tuning.crosshair_size);
    if let Ok((state, presentation, mut body)) = local.single_mut() {
        let canonical_eye = state.position + Vec3::Y * state.tuning.eye_height;
        let eye = level::world().clip_camera(
            canonical_eye,
            presentation.map_or(state.position, |position| position.0)
                + Vec3::Y * state.tuning.eye_height,
            tuning.camera_collision_radius,
        );
        if let Some(death) = state.death {
            // Quadratic ease-out, then hold until respawn.
            let progress = if tuning.death_camera_time == 0.0 {
                1.0
            } else {
                (death.total_ticks.saturating_sub(death.remaining_ticks) as f32
                    / (tuning.death_camera_time.max(f32::EPSILON)
                        * hookrunner_shared::TICK_HZ as f32))
                    .clamp(0.0, 1.0)
            };
            let eased = 1.0 - (1.0 - progress).powi(2);
            let end = eye
                + Quat::from_rotation_y(state.yaw)
                    * Vec3::new(
                        0.0,
                        tuning.death_camera_height,
                        tuning.death_camera_distance,
                    );
            camera.translation = level::world().clip_camera(
                eye,
                eye.lerp(end, eased),
                tuning.camera_collision_radius,
            );
            let start_rotation = Quat::from_euler(EulerRot::YXZ, state.yaw, death.pitch, 0.0);
            let target = state.position + Vec3::Y * (state.tuning.player_height / 2.0);
            let end_rotation = if end.distance_squared(target) > 1e-8 {
                Transform::from_translation(end)
                    .looking_at(target, Vec3::Y)
                    .rotation
            } else {
                start_rotation
            };
            camera.rotation = start_rotation.slerp(end_rotation, eased);
            // Reveal the body after the camera has cleared its head/near plane.
            *body = if camera.translation.distance(eye) > state.tuning.player_radius + 0.2 {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
            crosshair.display = Display::None;
            return;
        }
        *body = Visibility::Hidden;
        crosshair.display = if menu.open {
            Display::None
        } else {
            Display::Flex
        };
        camera.translation = eye;
        camera.rotation = Quat::from_euler(
            EulerRot::YXZ,
            look.yaw + state.view_yaw_offset,
            look.pitch,
            0.0,
        );
    }
}

fn update_hud(
    mut commands: Commands,
    rounds: Query<&hookrunner_shared::match_state::MatchState>,
    players: Query<(&PlayerId, &PlayerState, &PlayerColor), With<Predicted>>,
    stats: Res<NetworkStats>,
    session: Res<Session>,
    time: Res<Time<Real>>,
    mut elapsed: Local<f32>,
    mut hud: Single<(Entity, &mut Text), With<Hud>>,
    mut previous_spans: Local<Vec<(String, Option<PlayerColor>)>>,
) {
    *elapsed += time.delta_secs();
    if *elapsed < 0.1 {
        return;
    }
    *elapsed = 0.0;
    let ping = stats
        .ping_ms
        .map(|ms| format!("{ms:.0}"))
        .unwrap_or_else(|| "—".into());
    let fps = if stats.frame_ms > 0.0 {
        1000.0 / stats.frame_ms
    } else {
        0.0
    };
    let label = if session.is_playing() {
        format!("{fps:.0} fps, {ping} ms\n")
    } else {
        String::new()
    };
    let mut spans = Vec::new();
    if session.is_playing() {
        if let Some((_, state, color)) = players.iter().next() {
            spans.push((session.nickname.clone(), Some(*color)));
            spans.push((
                format!("\nHP: {} / {}", state.health.0, state.tuning.max_health),
                Some(*color),
            ));
            if let Some(death) = state.death {
                if state.match_paused {
                    spans.push((" | Waiting for next match".into(), None));
                } else {
                    let seconds =
                        (death.remaining_ticks as f64 / hookrunner_shared::TICK_HZ).ceil() as u32;
                    spans.push((format!(" | Respawn in {seconds}s"), None));
                }
            }
        }
        if let Some(round) = rounds.iter().next() {
            let remaining = round.remaining_seconds;
            let phase = if round.results {
                "Next match"
            } else {
                "Time left"
            };
            spans.push((
                format!("\n{phase} {:02}:{:02}\n", remaining / 60, remaining % 60),
                None,
            ));
            let ranked = round.ranked();
            for (rank, row) in ranked.iter().take(3).enumerate() {
                spans.push((format!("\n{}. ", rank + 1), None));
                spans.push((row.nickname.clone(), Some(row.color)));
                spans.push((format!("  {}K / {}D", row.kills, row.deaths), None));
            }
            let own = players.iter().next().map(|(id, _, _)| id.0);
            if let Some((rank, row)) = ranked
                .iter()
                .enumerate()
                .find(|(_, row)| Some(row.id) == own)
            {
                spans.push((format!("\nYou: #{} ", rank + 1), None));
                spans.push((row.nickname.clone(), Some(row.color)));
                spans.push((format!("  {}K / {}D", row.kills, row.deaths), None));
            }
        }
    }
    if hud.1.0 != label {
        hud.1.0 = label;
    }
    if *previous_spans != spans {
        commands.entity(hud.0).despawn_related::<Children>();
        commands.entity(hud.0).with_children(|parent| {
            for (segment, color) in &spans {
                let mut span = parent.spawn(TextSpan::new(segment.clone()));
                if let Some(color) = color {
                    let [r, g, b] = color.rgb();
                    span.insert(TextColor(Color::srgb(r, g, b)));
                }
            }
        });
        *previous_spans = spans;
    }
}
