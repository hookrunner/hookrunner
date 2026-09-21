use bevy::{
    camera::{Exposure, visibility::RenderLayers},
    core_pipeline::tonemapping::Tonemapping,
    ecs::system::SystemParam,
    light::NotShadowCaster,
    prelude::*,
    scene::SceneInstanceReady,
    transform::helper::TransformHelper,
};
use hookrunner_shared::{
    PlayerId, PlayerInput, PlayerState, level,
    weapon::{self, Projectile},
};
use lightyear::prelude::{input::native::ActionState, *};

const VIEW_LAYER: usize = 1;
const REST_POSITION: Vec3 = Vec3::new(0.23, -0.20, -0.46);
const MUZZLE_CLEARANCE: f32 = 0.015;
const FLASH_HALF_LENGTH: f32 = 0.055;

pub struct WeaponPlugin;
impl Plugin for WeaponPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Feedback>()
            .add_systems(Startup, setup)
            .add_systems(
                Update,
                (
                    detect_shots,
                    animate_weapon,
                    local_shots,
                    attach_bolts,
                    advance_local_bolts,
                    sync_bolts,
                    fade_impacts,
                )
                    .chain()
                    .after(InterpolationSystems::Interpolate)
                    .after(crate::view::CameraUpdated),
            );
    }
}

#[derive(Resource)]
pub(crate) struct WeaponAssets {
    pub scene: Handle<Scene>,
    beam: Handle<Mesh>,
    spark: Handle<Mesh>,
    cyan: Handle<StandardMaterial>,
    core: Handle<StandardMaterial>,
    glow: Handle<StandardMaterial>,
}
#[derive(Resource, Default)]
struct Feedback {
    player: Option<u64>,
    shot: u16,
    recoil: f32,
    flash: f32,
    pending_shot: bool,
}
#[derive(Component)]
struct ViewWeapon;
#[derive(Component)]
struct MuzzleFlash;
#[derive(Component)]
struct MuzzleSocket;
#[derive(Component)]
struct WeaponCamera;
#[derive(Component)]
struct BoltVisual;
#[derive(Component)]
struct LocalBolt {
    bolt: Projectile,
    age: f32,
}
#[derive(Component)]
struct ImpactFlash {
    age: f32,
}

fn setup(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let weapon = WeaponAssets {
        scene: assets.load(
            GltfAssetLabel::Scene(0).from_asset("weapons/starter_pistol/built/starter_pistol.glb"),
        ),
        beam: meshes.add(Cuboid::new(1., 1., 1.)),
        spark: meshes.add(Sphere::new(1.).mesh().ico(1).unwrap()),
        cyan: materials.add(StandardMaterial {
            base_color: Color::srgb(0.45, 0.85, 1.),
            unlit: true,
            ..default()
        }),
        core: materials.add(StandardMaterial {
            base_color: Color::srgb(0.82, 0.97, 1.),
            unlit: true,
            ..default()
        }),
        glow: materials.add(StandardMaterial {
            base_color: Color::srgba(0.25, 0.72, 1., 0.5),
            unlit: true,
            alpha_mode: AlphaMode::Add,
            ..default()
        }),
    };
    // A separate depth buffer keeps the first-person gun out of nearby walls.
    // This camera also draws the existing crosshair/FPS UI above the weapon.
    commands.spawn((
        Camera3d::default(),
        WeaponCamera,
        Camera {
            order: 1,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        Projection::Perspective(PerspectiveProjection {
            fov: 70_f32.to_radians(),
            near: 0.01,
            ..default()
        }),
        Exposure::INDOOR,
        // Keep weapon fill independent of the map. The coated materials limit
        // black-paint reflections while this fill keeps the surface wear visible.
        AmbientLight {
            brightness: 450.,
            ..default()
        },
        Tonemapping::None,
        Transform::default(),
        RenderLayers::layer(VIEW_LAYER),
        Msaa::Sample4,
        IsDefaultUiCamera,
    ));
    commands
        .spawn((
            SceneRoot(weapon.scene.clone()),
            Transform::from_translation(REST_POSITION),
            Visibility::Hidden,
            ViewWeapon,
        ))
        .observe(prepare_view_model);
    for (position, intensity, color) in [
        (Vec3::new(-0.5, 0.7, 0.2), 35., Color::srgb(0.8, 0.9, 1.)),
        (Vec3::new(0.8, 0.1, -0.5), 20., Color::srgb(0.5, 0.75, 1.)),
    ] {
        commands.spawn((
            PointLight {
                intensity,
                color,
                shadows_enabled: false,
                range: 3.,
                ..default()
            },
            Transform::from_translation(position),
            RenderLayers::layer(VIEW_LAYER),
        ));
    }
    commands.insert_resource(weapon);
}

fn prepare_view_model(
    event: On<SceneInstanceReady>,
    children: Query<&Children>,
    names: Query<&Name>,
    mut commands: Commands,
    assets: Res<WeaponAssets>,
) {
    for entity in children.iter_descendants(event.entity) {
        commands
            .entity(entity)
            .insert((RenderLayers::layer(VIEW_LAYER), NotShadowCaster));
        if names
            .get(entity)
            .is_ok_and(|name| name.as_str() == "Muzzle")
        {
            commands.entity(entity).insert(MuzzleSocket).with_child((
                Mesh3d(assets.spark.clone()),
                MeshMaterial3d(assets.core.clone()),
                // The sphere's rear edge must also clear the barrel lip.
                Transform::from_xyz(0., 0., -(MUZZLE_CLEARANCE + FLASH_HALF_LENGTH))
                    .with_scale(Vec3::new(0.03, 0.03, FLASH_HALF_LENGTH)),
                Visibility::Hidden,
                RenderLayers::layer(VIEW_LAYER),
                NotShadowCaster,
                MuzzleFlash,
            ));
        }
    }
}

fn beam_children(parent: &mut ChildSpawnerCommands, assets: &WeaponAssets) {
    for (width, material) in [
        (0.16, &assets.glow),
        (0.08, &assets.cyan),
        (0.0325, &assets.core),
    ] {
        parent.spawn((
            Mesh3d(assets.beam.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_scale(Vec3::new(width, width, 1.)),
            NotShadowCaster,
        ));
    }
}

/// Read the authored socket through the current hierarchy, including recoil.
/// Both cameras share a viewport but have different FOVs: convert the socket
/// through their projection scales before placing a world-space projectile.
#[derive(SystemParam)]
struct MuzzleView<'w, 's> {
    sockets: Query<'w, 's, Entity, With<MuzzleSocket>>,
    transforms: TransformHelper<'w, 's>,
    world_camera:
        Query<'w, 's, (&'static Transform, &'static Projection), With<crate::view::PlayerCamera>>,
    weapon_camera: Query<'w, 's, &'static Projection, With<WeaponCamera>>,
}

impl MuzzleView<'_, '_> {
    fn shot_geometry(&self) -> Option<(Vec3, Vec3, Vec3)> {
        let socket = self
            .transforms
            .compute_global_transform(self.sockets.single().ok()?)
            .ok()?
            .transform_point(Vec3::NEG_Z * MUZZLE_CLEARANCE);
        let (camera, Projection::Perspective(world_projection)) =
            self.world_camera.single().ok()?
        else {
            return None;
        };
        let Projection::Perspective(weapon_projection) = self.weapon_camera.single().ok()? else {
            return None;
        };
        let scale = (world_projection.fov * 0.5).tan() / (weapon_projection.fov * 0.5).tan();
        let position = camera.transform_point(socket * Vec3::new(scale, scale, 1.));
        Some((position, camera.translation, *camera.forward()))
    }
}

fn detect_shots(
    mut feedback: ResMut<Feedback>,
    players: Query<(&PlayerId, &PlayerState), With<Predicted>>,
) {
    feedback.pending_shot = false;
    let Ok((id, state)) = players.single() else {
        feedback.player = None;
        return;
    };
    if feedback.player != Some(id.0) {
        feedback.player = Some(id.0);
        feedback.shot = state.weapon.shot;
        return;
    }
    let difference = state.weapon.shot.wrapping_sub(feedback.shot);
    // Reconciliation must not replay old muzzle flashes.
    if difference == 0 || difference >= 32768 {
        return;
    }
    feedback.shot = state.weapon.shot;
    if state.death.is_some() {
        return;
    }
    feedback.pending_shot = true;
    feedback.recoil = 1.;
    feedback.flash = 0.07;
}

fn local_shots(
    mut commands: Commands,
    assets: Res<WeaponAssets>,
    feedback: Res<Feedback>,
    muzzle: MuzzleView,
    targets: Query<(&PlayerId, &PlayerState)>,
    players: Query<(&PlayerId, &PlayerState, &ActionState<PlayerInput>), With<Predicted>>,
) {
    if !feedback.pending_shot {
        return;
    }
    let Ok((id, state, input)) = players.single() else {
        return;
    };
    // animate_weapon ran first, so the socket matches the pose drawn this frame.
    let Some((muzzle, eye, direction)) = muzzle.shot_geometry() else {
        return;
    };
    let mut bolt = Projectile::from_shot(id.0, state, &input.0);
    bolt.origin = eye;
    bolt.position = eye;
    bolt.direction = direction;
    // Pick the reticle target first, then send the visible bolt from the barrel
    // to that point. Its tail can never extend backwards into the gun.
    let range = weapon::PROJECTILE_SPEED * 2.;
    let fraction = weapon::trace(
        level::world(),
        &bolt,
        direction * range,
        targets.iter().map(|(id, state)| (id.0, state)),
    )
    .map_or(1., |(fraction, _)| fraction);
    let target = eye + direction * range * fraction;
    if (target - muzzle).dot(direction) <= 0. {
        spawn_impact(&mut commands, &assets, target, -direction);
        return;
    }
    // The view model can overlap walls. Never let that launch a cosmetic bolt
    // through a wall that is between the player and the visible muzzle.
    if let Some(fraction) =
        level::world().sweep_sphere(eye, muzzle - eye, weapon::PROJECTILE_RADIUS)
    {
        spawn_impact(
            &mut commands,
            &assets,
            eye + (muzzle - eye) * fraction,
            -direction,
        );
        return;
    }
    bolt.origin = muzzle;
    bolt.position = muzzle;
    bolt.direction = (target - muzzle).normalize_or_zero();
    commands
        .spawn((
            LocalBolt { bolt, age: 0. },
            Transform::default(),
            Visibility::Inherited,
        ))
        .with_children(|parent| beam_children(parent, &assets));
}

type UnrenderedBolt = (With<Projectile>, With<Interpolated>, Without<BoltVisual>);

fn attach_bolts(
    mut commands: Commands,
    assets: Res<WeaponAssets>,
    bolts: Query<Entity, UnrenderedBolt>,
) {
    for entity in &bolts {
        commands
            .entity(entity)
            .insert((BoltVisual, Transform::default(), Visibility::Inherited))
            .with_children(|parent| beam_children(parent, &assets));
    }
}

fn bolt_transform(bolt: &Projectile) -> Transform {
    let distance = bolt.position.distance(bolt.origin);
    let length = distance.clamp(0.02, weapon::PROJECTILE_LENGTH);
    Transform::from_translation(bolt.position - bolt.direction * length / 2.)
        .looking_to(bolt.direction, Vec3::Y)
        .with_scale(Vec3::new(1., 1., length))
}

fn advance_local_bolts(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<WeaponAssets>,
    mut bolts: Query<(Entity, &mut LocalBolt, &mut Transform)>,
    players: Query<(&PlayerId, &PlayerState)>,
) {
    for (entity, mut local, mut transform) in &mut bolts {
        let delta = local.bolt.direction * weapon::PROJECTILE_SPEED * time.delta_secs();
        if let Some((fraction, _)) = weapon::trace(
            level::world(),
            &local.bolt,
            delta,
            players.iter().map(|(id, state)| (id.0, state)),
        ) {
            spawn_impact(
                &mut commands,
                &assets,
                local.bolt.position + delta * fraction,
                -local.bolt.direction,
            );
            commands.entity(entity).despawn();
            continue;
        }
        local.bolt.position += delta;
        local.age += time.delta_secs();
        if local.age >= 2. {
            commands.entity(entity).despawn();
            continue;
        }
        *transform = bolt_transform(&local.bolt);
    }
}

fn sync_bolts(
    feedback: Res<Feedback>,
    mut bolts: Query<(&Projectile, &mut Transform, &mut Visibility), With<BoltVisual>>,
) {
    for (bolt, mut transform, mut visibility) in &mut bolts {
        *visibility = if feedback.player == Some(bolt.owner) {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        *transform = bolt_transform(bolt);
    }
}

fn animate_weapon(
    time: Res<Time>,
    mut feedback: ResMut<Feedback>,
    local: Query<&PlayerState, With<Predicted>>,
    mut gun: Single<(&mut Transform, &mut Visibility), With<ViewWeapon>>,
    mut flashes: Query<&mut Visibility, (With<MuzzleFlash>, Without<ViewWeapon>)>,
) {
    let alive = local.single().is_ok_and(|state| state.death.is_none());
    *gun.1 = if alive {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    gun.0.translation = REST_POSITION + Vec3::new(0., 0.012, 0.055) * feedback.recoil;
    gun.0.rotation = Quat::from_euler(
        EulerRot::YXZ,
        0.08,
        0.025 + 0.16 * feedback.recoil,
        -0.035 * feedback.recoil,
    );
    for mut flash in &mut flashes {
        *flash = if alive && feedback.flash > 0. {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    feedback.recoil *= (-18. * time.delta_secs()).exp();
    feedback.flash = (feedback.flash - time.delta_secs()).max(0.);
}

#[derive(Component)]
enum SplashElement {
    Core,
    Halo,
    Ray(Vec3),
}

fn spawn_impact(commands: &mut Commands, assets: &WeaponAssets, position: Vec3, normal: Vec3) {
    let rotation = Quat::from_rotation_arc(Vec3::Z, normal);
    commands
        .spawn((
            ImpactFlash { age: 0. },
            Transform::from_translation(position + normal * 0.06),
            Visibility::Inherited,
        ))
        .with_children(|parent| {
            for (element, material, size) in [
                (SplashElement::Halo, &assets.glow, 0.32),
                (SplashElement::Core, &assets.core, 0.18),
            ] {
                parent.spawn((
                    Mesh3d(assets.spark.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_scale(Vec3::splat(size)),
                    element,
                    NotShadowCaster,
                ));
            }
            for i in 0..8 {
                let angle = i as f32 * std::f32::consts::TAU / 8.;
                let direction = rotation * Vec3::new(angle.cos(), angle.sin(), 0.45).normalize();
                parent.spawn((
                    Mesh3d(assets.beam.clone()),
                    MeshMaterial3d(assets.cyan.clone()),
                    Transform::default(),
                    SplashElement::Ray(direction),
                    NotShadowCaster,
                ));
            }
        });
}

fn fade_impacts(
    mut commands: Commands,
    time: Res<Time>,
    mut impacts: Query<(Entity, &mut ImpactFlash, &Children)>,
    mut elements: Query<(&SplashElement, &mut Transform)>,
) {
    for (entity, mut flash, children) in &mut impacts {
        flash.age += time.delta_secs();
        if flash.age >= 0.3 {
            commands.entity(entity).despawn();
            continue;
        }
        let progress = flash.age / 0.3;
        let fade = 1. - progress;
        for child in children {
            let Ok((element, mut transform)) = elements.get_mut(*child) else {
                continue;
            };
            match element {
                SplashElement::Core => transform.scale = Vec3::splat(0.18 * fade * fade),
                SplashElement::Halo => {
                    transform.scale = Vec3::splat((0.32 + progress * 0.6) * fade.sqrt())
                }
                SplashElement::Ray(direction) => {
                    *transform = Transform::from_translation(*direction * progress * 1.15)
                        .looking_to(*direction, Vec3::Y)
                        .with_scale(Vec3::new(0.055 * fade, 0.055 * fade, 0.28 * fade));
                }
            }
        }
    }
}
