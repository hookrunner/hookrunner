//! Screen-space labels projected from the replicated player positions.
use bevy::{prelude::*, transform::TransformSystems, window::PrimaryWindow};
use hookrunner_client::Session;
use hookrunner_shared::{
    PlayerId, PlayerState, arena, health::MAX_HEALTH, level, player_color::PlayerColor,
    protocol::PlayerName,
};
use lightyear::prelude::Predicted;
use std::collections::HashMap;

pub struct NameplatesPlugin;
impl Plugin for NameplatesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostUpdate, sync.after(TransformSystems::Propagate));
    }
}

#[derive(Component)]
struct Nameplate(Entity);

#[derive(Component)]
struct HealthText {
    player: Entity,
    value: u16,
}

#[derive(Component)]
struct HealthBar(Entity);

fn plate_scale(distance: f32) -> f32 {
    (10.0 / distance.max(f32::EPSILON)).max(0.32)
}

fn plate_top(anchor_y: f32, scale: f32) -> f32 {
    // UiTransform scales around the label center. Keep its lower edge near the head.
    anchor_y - 19.0 * (1.0 + scale) - 18.0 * scale
}

fn sync(
    mut commands: Commands,
    session: Res<Session>,
    time: Res<Time<Real>>,
    mut visibility: Local<HashMap<Entity, bool>>,
    mut elapsed: Local<f32>,
    camera: Single<(&Camera, &GlobalTransform), With<crate::view::PlayerCamera>>,
    window: Single<&Window, With<PrimaryWindow>>,
    players: Query<
        (Entity, &PlayerName, &PlayerColor, &PlayerState),
        (With<PlayerId>, Without<Predicted>),
    >,
    mut labels: Query<(Entity, &Nameplate, &mut Node, &mut UiTransform), Without<HealthBar>>,
    mut health_texts: Query<(&mut HealthText, &mut Text)>,
    mut health_bars: Query<(&HealthBar, &mut Node), Without<Nameplate>>,
) {
    *elapsed += time.delta_secs();
    let check_walls = *elapsed >= 0.1;
    if check_walls {
        *elapsed = 0.0;
    }
    for (entity, name, color, state) in &players {
        let world = state.position + Vec3::Y * (arena::PLAYER_HEIGHT + 0.35);
        let line = world - camera.1.translation();
        let distance = line.length();
        let scale = plate_scale(distance);
        let visible = session.is_playing() && state.death.is_none() && distance < 45.0;
        let unblocked = if visible {
            if check_walls || !visibility.contains_key(&entity) {
                let clear = level::world()
                    .sweep_sphere(camera.1.translation(), line, 0.02)
                    .is_none_or(|fraction| fraction >= 0.99);
                visibility.insert(entity, clear);
            }
            visibility.get(&entity).copied().unwrap_or(false)
        } else {
            false
        };
        let screen = (visible && unblocked)
            .then(|| camera.0.world_to_viewport(camera.1, world).ok())
            .flatten()
            .filter(|point| {
                point.x >= 0.0
                    && point.x <= window.width()
                    && point.y >= 0.0
                    && point.y <= window.height()
            });
        if let Some((_, _, mut node, mut transform)) =
            labels.iter_mut().find(|(_, plate, _, _)| plate.0 == entity)
        {
            node.display = if screen.is_some() {
                Display::Flex
            } else {
                Display::None
            };
            if let Some(point) = screen {
                node.left = px(point.x - 110.0);
                node.top = px(plate_top(point.y, scale));
                transform.scale = Vec2::splat(scale);
            }
            for (mut label, mut text) in &mut health_texts {
                if label.player == entity {
                    let health = state.health.0.min(MAX_HEALTH);
                    if label.value != health {
                        label.value = health;
                        text.0 = format!("{health} / {MAX_HEALTH}");
                    }
                    break;
                }
            }
            for (owner, mut bar) in &mut health_bars {
                if owner.0 == entity {
                    let width =
                        percent(100.0 * state.health.0.min(MAX_HEALTH) as f32 / MAX_HEALTH as f32);
                    if bar.width != width {
                        bar.width = width;
                    }
                    break;
                }
            }
        } else {
            let [r, g, b] = color.rgb();
            let position = screen.unwrap_or(Vec2::ZERO);
            let health = state.health.0.min(MAX_HEALTH);
            commands
                .spawn((
                    Nameplate(entity),
                    Node {
                        position_type: PositionType::Absolute,
                        display: if screen.is_some() {
                            Display::Flex
                        } else {
                            Display::None
                        },
                        left: px(position.x - 110.0),
                        top: px(plate_top(position.y, scale)),
                        width: px(220.0),
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        row_gap: px(2.0),
                        ..default()
                    },
                    UiTransform::from_scale(Vec2::splat(scale)),
                ))
                .with_children(|plate| {
                    plate.spawn((
                        Text::new(name.0.clone()),
                        TextFont {
                            font_size: 16.0,
                            ..default()
                        },
                        TextColor(Color::srgb(r, g, b)),
                    ));
                    plate
                        .spawn((
                            Node {
                                width: px(96.0),
                                height: px(6.0),
                                ..default()
                            },
                            BackgroundColor(Color::srgba(0.02, 0.03, 0.04, 0.85)),
                        ))
                        .with_children(|track| {
                            track.spawn((
                                HealthBar(entity),
                                Node {
                                    width: percent(100.0 * health as f32 / MAX_HEALTH as f32),
                                    height: percent(100.0),
                                    ..default()
                                },
                                BackgroundColor(Color::srgb(r, g, b)),
                            ));
                        });
                    plate.spawn((
                        HealthText {
                            player: entity,
                            value: health,
                        },
                        Text::new(format!("{health} / {MAX_HEALTH}")),
                        TextFont {
                            font_size: 12.0,
                            ..default()
                        },
                        TextColor(Color::srgb(r, g, b)),
                    ));
                });
        }
    }
    visibility.retain(|entity, _| players.contains(*entity));
    for (entity, plate, _, _) in &labels {
        if !players.contains(plate.0) {
            commands.entity(entity).despawn();
        }
    }
}
