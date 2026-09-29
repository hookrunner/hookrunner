//! Server-confirmed kills; rendering is shared by web and native clients.
use bevy::prelude::*;
use hookrunner_client::Session;
use hookrunner_shared::{match_state::MatchState, player_color::PlayerColor};

pub struct KillFeedPlugin;
impl Plugin for KillFeedPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup).add_systems(Update, present);
    }
}
#[derive(Component)]
struct KillFeed;
fn setup(mut commands: Commands) {
    commands.spawn((
        KillFeed,
        Text::new(""),
        TextFont {
            font_size: 16.0,
            ..default()
        },
        TextColor(Color::srgb(1.0, 0.83, 0.62)),
        TextLayout::new_with_justify(Justify::Right),
        Node {
            position_type: PositionType::Absolute,
            top: px(20),
            right: px(22),
            max_width: percent(45),
            ..default()
        },
    ));
}
fn present(
    mut commands: Commands,
    session: Res<Session>,
    rounds: Query<Ref<MatchState>>,
    mut text: Single<(Entity, &mut Text, &mut Node), With<KillFeed>>,
    mut previous_spans: Local<Vec<(String, Option<PlayerColor>)>>,
) {
    if !session.is_changed() && !rounds.iter().any(|round| round.is_changed()) {
        return;
    }
    let mut spans = Vec::new();
    if session.is_playing() {
        if let Some(round) = rounds.iter().next() {
            for (index, entry) in round.kill_feed.iter().rev().enumerate() {
                if index > 0 {
                    spans.push(("\n".into(), None));
                }
                if let Some(killer) = &entry.killer {
                    spans.push((killer.clone(), entry.killer_color));
                    spans.push((" > ".into(), None));
                } else {
                    spans.push(("Arena > ".into(), None));
                }
                spans.push((entry.victim.clone(), Some(entry.victim_color)));
            }
        }
    }
    if *previous_spans != spans {
        // Removing the last child also removes Children, which does not trigger
        // Bevy's Changed<Children> text rebuild. Mark the root text dirty too.
        text.1.0.clear();
        text.2.display = if spans.is_empty() {
            Display::None
        } else {
            Display::Flex
        };
        commands.entity(text.0).despawn_related::<Children>();
        commands.entity(text.0).with_children(|parent| {
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
