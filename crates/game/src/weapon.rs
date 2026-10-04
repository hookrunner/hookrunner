use bevy::{
    camera::{Exposure, visibility::RenderLayers},
    core_pipeline::tonemapping::Tonemapping,
    ecs::system::SystemParam,
    light::NotShadowCaster,
    prelude::*,
    scene::SceneInstanceReady,
    transform::helper::TransformHelper,
};
use hookrunner_client::{PredictedShot, PredictedShots};
use hookrunner_shared::{
    PlayerId, PlayerState, level,
    weapon::{self, Projectile, ShotImpact},
};
use lightyear::prelude::*;
use std::collections::VecDeque;

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
                    receive_impacts,
                    reconcile_bolts,
                    advance_local_bolts,
                    sync_bolts,
                    fade_impacts,
                    hit_feedback,
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
    recoil: f32,
    flash: f32,
    pending_shots: Vec<PredictedShot>,
    seen: VecDeque<(u64, u16, f64)>,
    finished: VecDeque<(u64, u16, f64)>,
    impacts: Vec<(ShotImpact, f64)>,
    hit_flash: f32,
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
    muzzle_offset: Vec3,
    correction: Vec3,
    confirmed_tick: Option<Tick>,
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
    mut shots: ResMut<PredictedShots>,
    time: Res<Time<Real>>,
    players: Query<(&PlayerId, &PlayerState), With<Predicted>>,
) {
    feedback.pending_shots.clear();
    let now = time.elapsed_secs_f64();
    feedback.seen.retain(|(_, _, at)| now - at < 4.0);
    feedback.finished.retain(|(_, _, at)| now - at < 4.0);
    let Ok((id, state)) = players.single() else {
        feedback.player = None;
        shots.0.clear();
        return;
    };
    if feedback.player != Some(id.0) {
        feedback.seen.clear();
        feedback.finished.clear();
        feedback.impacts.clear();
        feedback.player = Some(id.0);
    }
    for shot in shots.0.drain(..) {
        if shot.bolt.owner != id.0
            || feedback
                .seen
                .iter()
                .any(|(owner, number, _)| *owner == id.0 && *number == shot.bolt.shot)
        {
            continue;
        }
        feedback.seen.push_back((id.0, shot.bolt.shot, now));
        if state.death.is_none() {
            feedback.pending_shots.push(shot);
            feedback.recoil = 1.0;
            feedback.flash = 0.07;
        }
    }
}

fn local_shots(
    mut commands: Commands,
    assets: Res<WeaponAssets>,
    mut feedback: ResMut<Feedback>,
    timeline: Res<LocalTimeline>,
    muzzle: MuzzleView,
) {
    let geometry = muzzle.shot_geometry();
    for shot in feedback.pending_shots.drain(..) {
        let mut bolt = shot.bolt;
        let age = ((timeline.tick() - shot.tick) as f32).max(0.0)
            * hookrunner_shared::TICK_DURATION.as_secs_f32();
        if age >= 2.0 {
            continue;
        }
        let muzzle_offset = geometry.map_or(Vec3::ZERO, |(muzzle, _, _)| muzzle - bolt.origin);
        let advance = bolt.direction * weapon::PROJECTILE_SPEED * age;
        if level::world()
            .sweep_sphere(bolt.position, advance, weapon::PROJECTILE_RADIUS)
            .is_some()
        {
            continue;
        }
        bolt.position += advance;
        commands
            .spawn((
                LocalBolt {
                    bolt,
                    age,
                    muzzle_offset,
                    correction: Vec3::ZERO,
                    confirmed_tick: None,
                },
                Transform::default(),
                Visibility::Inherited,
            ))
            .with_children(|parent| beam_children(parent, &assets));
    }
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

fn receive_impacts(
    mut commands: Commands,
    time: Res<Time<Real>>,
    assets: Res<WeaponAssets>,
    mut feedback: ResMut<Feedback>,
    mut receivers: Query<&mut MessageReceiver<ShotImpact>, With<Connected>>,
    timelines: Query<&lightyear::interpolation::timeline::InterpolationTimeline, With<Connected>>,
    locals: Query<(Entity, &LocalBolt)>,
) {
    let now = time.elapsed_secs_f64();
    for mut receiver in &mut receivers {
        for impact in receiver.receive() {
            // Matching is by shot identity, including hits before the server
            // projectile was ever replicated.
            for (entity, local) in &locals {
                if (local.bolt.owner, local.bolt.shot) == (impact.owner, impact.shot) {
                    commands.entity(entity).despawn();
                }
            }
            if feedback.player == Some(impact.owner) && impact.victim.is_some() {
                feedback.hit_flash = 0.12;
            }
            feedback.impacts.push((impact, now));
        }
    }
    let tick = timelines.iter().next().map(|timeline| timeline.tick());
    let owner = feedback.player;
    let mut completed = Vec::new();
    feedback.impacts.retain(|(impact, received)| {
        // Other players and their splashes use the same presentation time.
        if owner == Some(impact.owner)
            || tick.is_some_and(|tick| tick >= Tick(impact.tick))
            || now - received > 0.5
        {
            spawn_impact(&mut commands, &assets, impact.position, impact.normal);
            completed.push((impact.owner, impact.shot, now));
            false
        } else {
            true
        }
    });
    feedback.finished.extend(completed);
}

fn reconcile_bolts(
    timeline: Res<LocalTimeline>,
    authoritative: Query<(&Confirmed<Projectile>, &ConfirmedTick), With<Interpolated>>,
    mut locals: Query<&mut LocalBolt>,
) {
    for mut local in &mut locals {
        let Some((confirmed, tick)) = authoritative.iter().find(|(confirmed, _)| {
            (confirmed.0.owner, confirmed.0.shot) == (local.bolt.owner, local.bolt.shot)
        }) else {
            continue;
        };
        if local.confirmed_tick == Some(tick.tick) {
            continue;
        }
        let lead = ((timeline.tick() - tick.tick) as f32).clamp(0.0, 30.0)
            * hookrunner_shared::TICK_DURATION.as_secs_f32();
        let position =
            confirmed.0.position + confirmed.0.direction * weapon::PROJECTILE_SPEED * lead;
        let difference = local.bolt.position - position;
        local.correction = if difference.length() < 3.0 {
            local.correction + difference
        } else {
            Vec3::ZERO
        };
        local.bolt = confirmed.0.clone();
        local.bolt.position = position;
        local.age = (weapon::PROJECTILE_LIFETIME_TICKS - local.bolt.remaining_ticks) as f32
            * hookrunner_shared::TICK_DURATION.as_secs_f32()
            + lead;
        local.confirmed_tick = Some(tick.tick);
    }
}

fn advance_local_bolts(
    mut commands: Commands,
    time: Res<Time>,
    mut bolts: Query<(Entity, &mut LocalBolt, &mut Transform)>,
) {
    for (entity, mut local, mut transform) in &mut bolts {
        let delta = local.bolt.direction * weapon::PROJECTILE_SPEED * time.delta_secs();
        // Predicted wall occlusion is cosmetic. Player hits and impact splashes
        // are exclusively confirmed by the server.
        if level::world()
            .sweep_sphere(local.bolt.position, delta, weapon::PROJECTILE_RADIUS)
            .is_some()
        {
            commands.entity(entity).despawn();
            continue;
        }
        local.bolt.position += delta;
        local.age += time.delta_secs();
        local.correction *= (-35.0 * time.delta_secs()).exp();
        if local.age >= 2.0 {
            commands.entity(entity).despawn();
            continue;
        }
        let muzzle_weight =
            (1.0 - local.bolt.position.distance(local.bolt.origin) / 4.0).clamp(0.0, 1.0);
        let offset = local.correction + local.muzzle_offset * muzzle_weight;
        *transform = bolt_transform(&local.bolt);
        transform.translation += offset;
    }
}

fn hit_feedback(
    time: Res<Time>,
    mut feedback: ResMut<Feedback>,
    mut crosshair: Single<&mut BackgroundColor, With<crate::view::Crosshair>>,
) {
    crosshair.0 = if feedback.hit_flash > 0.0 {
        Color::srgb(0.45, 0.85, 1.0)
    } else {
        Color::WHITE
    };
    feedback.hit_flash = (feedback.hit_flash - time.delta_secs()).max(0.0);
}

fn sync_bolts(
    feedback: Res<Feedback>,
    mut bolts: Query<(&Projectile, &mut Transform, &mut Visibility), With<BoltVisual>>,
) {
    for (bolt, mut transform, mut visibility) in &mut bolts {
        *visibility = if (feedback.player == Some(bolt.owner)
            && feedback
                .seen
                .iter()
                .any(|(owner, shot, _)| (*owner, *shot) == (bolt.owner, bolt.shot)))
            || feedback
                .finished
                .iter()
                .any(|(owner, shot, _)| (*owner, *shot) == (bolt.owner, bolt.shot))
        {
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
