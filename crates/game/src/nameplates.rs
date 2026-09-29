//! Screen-space labels projected from the replicated player positions.
use bevy::{prelude::*, transform::TransformSystems, window::PrimaryWindow};
use hookrunner_client::Session;
use hookrunner_shared::{
    PlayerId, PlayerState, arena, level, player_color::PlayerColor, protocol::PlayerName,
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
    mut labels: Query<(Entity, &Nameplate, &mut Node)>,
) {
    *elapsed += time.delta_secs();
    let check_walls = *elapsed >= 0.1;
    if check_walls {
        *elapsed = 0.0;
    }
    for (entity, name, color, state) in &players {
        let world = state.position + Vec3::Y * (arena::PLAYER_HEIGHT + 0.35);
        let line = world - camera.1.translation();
        let visible =
            session.is_playing() && state.death.is_none() && line.length_squared() < 45.0 * 45.0;
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
        if let Some((_, _, mut node)) = labels.iter_mut().find(|(_, plate, _)| plate.0 == entity) {
            node.display = if screen.is_some() {
                Display::Flex
            } else {
                Display::None
            };
            if let Some(point) = screen {
                node.left = px(point.x - 110.0);
                node.top = px(point.y - 34.0);
            }
        } else {
            let [r, g, b] = color.rgb();
            let position = screen.unwrap_or(Vec2::ZERO);
            commands.spawn((
                Nameplate(entity),
                Text::new(name.0.clone()),
                TextFont {
                    font_size: 16.0,
                    ..default()
                },
                TextColor(Color::srgb(r, g, b)),
                TextLayout::new_with_justify(Justify::Center),
                Node {
                    position_type: PositionType::Absolute,
                    display: if screen.is_some() {
                        Display::Flex
                    } else {
                        Display::None
                    },
                    left: px(position.x - 110.0),
                    top: px(position.y - 34.0),
                    width: px(220.0),
                    ..default()
                },
            ));
        }
    }
    visibility.retain(|entity, _| players.contains(*entity));
    for (entity, plate, _) in &labels {
        if !players.contains(plate.0) {
            commands.entity(entity).despawn();
        }
    }
}
