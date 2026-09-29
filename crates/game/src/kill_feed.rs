//! Server-confirmed kills; rendering is shared by web and native clients.
use bevy::prelude::*;
use hookrunner_client::Session;
use hookrunner_shared::match_state::MatchState;

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
    session: Res<Session>,
    rounds: Query<Ref<MatchState>>,
    mut text: Single<&mut Text, With<KillFeed>>,
) {
    if !session.is_changed() && !rounds.iter().any(|round| round.is_changed()) {
        return;
    }
    let label = if session.is_playing() {
        rounds
            .iter()
            .next()
            .map(|round| {
                round
                    .kill_feed
                    .iter()
                    .rev()
                    .map(|entry| match &entry.killer {
                        Some(killer) => format!("{killer} > {}", entry.victim),
                        None => format!("Arena > {}", entry.victim),
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default()
    } else {
        String::new()
    };
    if text.0 != label {
        text.0 = label;
    }
}
