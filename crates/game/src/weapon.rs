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
    weapon::{self, Projectile, ProjectileHit},
};
use lightyear::prelude::{input::native::ActionState, *};

const VIEW_LAYER: usize = 1;
const REST_POSITION: Vec3 = Vec3::new(0.23, -0.20, -0.46);
const MUZZLE_CLEARANCE: f32 = 0.015;
const FLASH_HALF_LENGTH: f32 = 0.035;
const IMPACT_RADIUS: f32 = 0.32;
const BOLT_START_RADIUS: f32 = 0.012;
const BOLT_MAX_RADIUS: f32 = 0.12;
const BOLT_GROW_DISTANCE: f32 = 12.0;

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
                    confirmed_hits,
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
enum BoltPart {
    Head(f32),
    Trail(f32),
}
#[derive(Component)]
struct LocalBolt {
    bolt: Projectile,
    age: f32,
    muzzle: Vec3,
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
        spark: meshes.add(Sphere::new(1.).mesh().ico(2).unwrap()),
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
                    .with_scale(Vec3::new(0.014, 0.014, FLASH_HALF_LENGTH)),
                Visibility::Hidden,
                RenderLayers::layer(VIEW_LAYER),
                NotShadowCaster,
                MuzzleFlash,
            ));
        }
    }
}

fn beam_children(parent: &mut ChildSpawnerCommands, assets: &WeaponAssets, length: f32) {
    for (scale, material) in [
        (1.0, &assets.glow),
        (0.5, &assets.cyan),
        (0.2, &assets.core),
    ] {
        let width = bolt_trail_width(BOLT_START_RADIUS) * scale;
        parent.spawn((
            BoltPart::Trail(scale),
            Mesh3d(assets.beam.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_scale(Vec3::new(width, width, 1.)),
            NotShadowCaster,
        ));
    }
    for (scale, material) in [(1.0, &assets.glow), (0.45, &assets.core)] {
        let radius = BOLT_START_RADIUS * scale;
        parent.spawn((
            BoltPart::Head(scale),
            Mesh3d(assets.spark.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_xyz(0.0, 0.0, -0.5).with_scale(Vec3::new(
                radius,
                radius,
                radius / length,
            )),
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
    feedback.flash = 0.032;
}

fn local_shots(
    mut commands: Commands,
    assets: Res<WeaponAssets>,
    feedback: Res<Feedback>,
    muzzle: MuzzleView,
    players: Query<(&PlayerId, &PlayerState, &ActionState<PlayerInput>), With<Predicted>>,
) {
    if !feedback.pending_shot {
        return;
    }
    let Ok((id, state, input)) = players.single() else {
        return;
    };
    // animate_weapon ran first, so the socket matches the pose drawn this frame.
    let Some((muzzle, _, _)) = muzzle.shot_geometry() else {
        return;
    };
    let bolt = Projectile::from_shot(id.0, state, &input.0);
    let visual = Projectile {
        origin: muzzle,
        position: muzzle,
        ..bolt.clone()
    };
    let length = bolt_length(&visual);
    let transform = bolt_transform(&visual);
    commands
        .spawn((
            LocalBolt {
                bolt,
                age: 0.,
                muzzle,
            },
            transform,
            Visibility::Inherited,
        ))
        .with_children(|parent| beam_children(parent, &assets, length));
}

type UnrenderedBolt = (With<Projectile>, With<Interpolated>, Without<BoltVisual>);

fn attach_bolts(
    mut commands: Commands,
    assets: Res<WeaponAssets>,
    bolts: Query<(Entity, &Projectile), UnrenderedBolt>,
) {
    for (entity, bolt) in &bolts {
        commands
            .entity(entity)
            .insert((BoltVisual, bolt_transform(bolt), Visibility::Inherited))
            .with_children(|parent| beam_children(parent, &assets, bolt_length(bolt)));
    }
}

fn bolt_length(bolt: &Projectile) -> f32 {
    bolt.position
        .distance(bolt.origin)
        .clamp(0.02, weapon::PROJECTILE_LENGTH)
}

fn bolt_visual_radius(bolt: &Projectile) -> f32 {
    let progress = (bolt.position.distance(bolt.origin) / BOLT_GROW_DISTANCE).clamp(0.0, 1.0);
    let eased = progress * (2.0 - progress);
    BOLT_START_RADIUS + (BOLT_MAX_RADIUS - BOLT_START_RADIUS) * eased
}

fn bolt_trail_width(radius: f32) -> f32 {
    // The trail is an afterimage; only the leading sphere participates in hits.
    0.024 + radius * 0.75
}

fn resize_bolt_parts(
    children: &Children,
    parts: &mut Query<(&BoltPart, &mut Transform), (Without<LocalBolt>, Without<BoltVisual>)>,
    radius: f32,
    length: f32,
) {
    for child in children {
        if let Ok((part, mut transform)) = parts.get_mut(*child) {
            match part {
                BoltPart::Head(scale) => {
                    let radius = radius * scale;
                    transform.scale = Vec3::new(radius, radius, radius / length);
                }
                BoltPart::Trail(scale) => {
                    let width = bolt_trail_width(radius) * scale;
                    transform.scale.x = width;
                    transform.scale.y = width;
                }
            }
        }
    }
}

fn bolt_transform(bolt: &Projectile) -> Transform {
    let length = bolt_length(bolt);
    Transform::from_translation(bolt.position - bolt.direction * length / 2.)
        .looking_to(bolt.direction, Vec3::Y)
        .with_scale(Vec3::new(1., 1., length))
}

fn advance_local_bolts(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<WeaponAssets>,
    mut bolts: Query<(Entity, &mut LocalBolt, &mut Transform, &Children), Without<BoltPart>>,
    mut parts: Query<(&BoltPart, &mut Transform), (Without<LocalBolt>, Without<BoltVisual>)>,
) {
    for (entity, mut local, mut transform, children) in &mut bolts {
        let delta = local.bolt.direction * weapon::PROJECTILE_SPEED * time.delta_secs();
        let radius = local.bolt.radius_at(local.bolt.position + delta);
        if let Some(fraction) = level::world().sweep_sphere(local.bolt.position, delta, radius) {
            let position = local.bolt.position + delta * fraction;
            spawn_impact(
                &mut commands,
                &assets,
                position,
                -local.bolt.direction,
                local.bolt.radius_at(position),
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
        let visual = Projectile {
            origin: local.muzzle,
            direction: (local.bolt.position - local.muzzle).normalize_or(local.bolt.direction),
            ..local.bolt.clone()
        };
        *transform = bolt_transform(&visual);
        let length = bolt_length(&visual);
        let radius = bolt_visual_radius(&local.bolt);
        resize_bolt_parts(children, &mut parts, radius, length);
    }
}

fn confirmed_hits(
    mut commands: Commands,
    assets: Res<WeaponAssets>,
    mut connections: Query<&mut MessageReceiver<ProjectileHit>, (With<Client>, With<Connected>)>,
    local_bolts: Query<(Entity, &LocalBolt)>,
) {
    for mut connection in &mut connections {
        for hit in connection.receive() {
            for (entity, local) in &local_bolts {
                if local.bolt.owner == hit.owner && local.bolt.shot == hit.shot {
                    commands.entity(entity).despawn();
                }
            }
            spawn_impact(&mut commands, &assets, hit.position, hit.normal, hit.radius);
        }
    }
}

fn sync_bolts(
    feedback: Res<Feedback>,
    mut bolts: Query<
        (&Projectile, &mut Transform, &mut Visibility, &Children),
        (With<BoltVisual>, Without<BoltPart>),
    >,
    mut parts: Query<(&BoltPart, &mut Transform), (Without<LocalBolt>, Without<BoltVisual>)>,
) {
    for (bolt, mut transform, mut visibility, children) in &mut bolts {
        *visibility = if feedback.player == Some(bolt.owner) {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        *transform = bolt_transform(bolt);
        let length = bolt_length(bolt);
        let radius = bolt_visual_radius(bolt);
        resize_bolt_parts(children, &mut parts, radius, length);
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

fn spawn_impact(
    commands: &mut Commands,
    assets: &WeaponAssets,
    position: Vec3,
    normal: Vec3,
    radius: f32,
) {
    let rotation = Quat::from_rotation_arc(Vec3::Z, normal);
    commands
        .spawn((
            ImpactFlash { age: 0. },
            Transform::from_translation(position - normal * (radius - 0.06).max(0.0)),
            Visibility::Inherited,
        ))
        .with_children(|parent| {
            for (element, material, size) in [
                (SplashElement::Halo, &assets.glow, IMPACT_RADIUS),
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
                    transform.scale = Vec3::splat((IMPACT_RADIUS + progress * 0.6) * fade.sqrt())
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
