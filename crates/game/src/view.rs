use bevy::{
    camera::CameraOutputMode,
    core_pipeline::tonemapping::Tonemapping,
    input::mouse::AccumulatedMouseMotion,
    prelude::*,
    window::{CursorOptions, PrimaryWindow},
};
use hookrunner_client::NetworkStats;
use hookrunner_shared::{PlayerId, PlayerInput, PlayerState, arena, level, movement};
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
            .add_systems(Update, crate::map::finish_loading)
            .add_systems(PreUpdate, look_input.after(bevy::input::InputSystems))
            .add_systems(
                FixedPreUpdate,
                buffer_input.in_set(InputSystems::WriteClientInputs),
            )
            .add_systems(
                Update,
                (
                    attach_visuals,
                    sync_bodies,
                    follow_camera.in_set(CameraUpdated),
                    update_hud,
                )
                    .chain()
                    .after(InterpolationSystems::Interpolate),
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
pub(crate) struct PlayerCamera;
#[derive(Component)]
struct Hud;
#[derive(Component)]
struct Crosshair;

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
        Transform::from_translation(arena::spawn(0).position + Vec3::Y * arena::EYE_HEIGHT)
            .with_rotation(Quat::from_rotation_y(arena::spawn(0).yaw)),
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
    players: Query<(Entity, Has<Predicted>), PlayerWithoutVisual>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (entity, local) in &players {
        commands.entity(entity).insert((
            PlayerVisual,
            Transform::default(),
            if local {
                Visibility::Hidden
            } else {
                Visibility::Inherited
            },
        ));
        let color = materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.28, 0.035),
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
                    arena::PLAYER_RADIUS,
                    arena::PLAYER_HEIGHT - 2.0 * arena::PLAYER_RADIUS,
                ))),
                MeshMaterial3d(color),
                Transform::from_xyz(0.0, arena::PLAYER_HEIGHT / 2.0, 0.0),
            ));
            parent.spawn((
                Mesh3d(meshes.add(Cuboid::new(0.55, 0.18, 0.16))),
                MeshMaterial3d(visor),
                Transform::from_xyz(0.0, 1.45, -0.34),
            ));
        });
    }
}

pub(crate) fn look_input(
    mut look: ResMut<Look>,
    motion: Res<AccumulatedMouseMotion>,
    keys: Res<ButtonInput<KeyCode>>,
    window: Single<(&Window, &CursorOptions), With<PrimaryWindow>>,
    local: Query<&PlayerState, With<Predicted>>,
) {
    look.locked = window.0.focused && crate::platform::pointer_locked(window.1);
    if look.locked {
        if keys.just_pressed(KeyCode::Space) {
            look.jump_press = look.jump_press.wrapping_add(1);
        }
        if keys.just_pressed(KeyCode::ShiftLeft) || keys.just_pressed(KeyCode::ShiftRight) {
            look.dash_press = look.dash_press.wrapping_add(1);
        }
        if !local.iter().any(|state| state.death.is_some()) {
            look.yaw = (look.yaw - motion.delta.x * 0.002).rem_euclid(std::f32::consts::TAU);
            look.pitch = (look.pitch - motion.delta.y * 0.002)
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

fn follow_camera(
    look: Res<Look>,
    mut local: Query<(&PlayerState, &mut Visibility), With<Predicted>>,
    mut camera: Single<&mut Transform, (With<PlayerCamera>, Without<PlayerState>)>,
    mut crosshair: Single<&mut Node, With<Crosshair>>,
) {
    if let Ok((state, mut body)) = local.single_mut() {
        let eye = state.position + Vec3::Y * arena::EYE_HEIGHT;
        if let Some(death) = state.death {
            // Quadratic ease-out: stop after two seconds, then hold until respawn.
            let progress = ((movement::RESPAWN_TICKS - death.remaining_ticks) as f32
                / (2.0 * hookrunner_shared::TICK_HZ as f32))
                .clamp(0.0, 1.0);
            let eased = 1.0 - (1.0 - progress).powi(2);
            let end = eye + Quat::from_rotation_y(state.yaw) * Vec3::new(0.0, 1.0, 4.0);
            camera.translation = level::world().clip_camera(eye, eye.lerp(end, eased), 0.15);
            let start_rotation = Quat::from_euler(EulerRot::YXZ, state.yaw, death.pitch, 0.0);
            let end_rotation = Transform::from_translation(end)
                .looking_at(
                    state.position + Vec3::Y * (arena::PLAYER_HEIGHT / 2.0),
                    Vec3::Y,
                )
                .rotation;
            camera.rotation = start_rotation.slerp(end_rotation, eased);
            // Reveal the body after the camera has cleared its head/near plane.
            *body = if camera.translation.distance(eye) > arena::PLAYER_RADIUS + 0.2 {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
            crosshair.display = Display::None;
            return;
        }
        *body = Visibility::Hidden;
        crosshair.display = Display::Flex;
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
    stats: Res<NetworkStats>,
    time: Res<Time<Real>>,
    mut elapsed: Local<f32>,
    mut hud: Single<&mut Text, With<Hud>>,
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
    let label = format!("{fps:.0} fps, {ping} ms");
    if hud.0 != label {
        hud.0 = label;
    }
}
