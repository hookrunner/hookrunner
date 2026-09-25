//! Match presentation lives in Bevy on both platforms.
use bevy::{input::mouse::MouseWheel, prelude::*, window::PrimaryWindow};
use hookrunner_client::Session;
use hookrunner_shared::{PlayerId, match_state::MatchState};
use lightyear::prelude::Predicted;

pub struct ScoreboardPlugin;
impl Plugin for ScoreboardPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup).add_systems(Update, present);
    }
}
#[derive(Component)]
struct Scoreboard;
#[derive(Component)]
struct ScoreboardText;

fn setup(mut commands: Commands) {
    commands
        .spawn((
            Scoreboard,
            GlobalZIndex(800),
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.03, 0.05, 0.88)),
        ))
        .with_children(|root| {
            root.spawn((
                ScoreboardText,
                Text::new(""),
                TextFont {
                    font_size: 18.0,
                    ..default()
                },
                TextColor(Color::srgb(0.88, 0.95, 0.96)),
                Node {
                    padding: UiRect::all(px(20)),
                    ..default()
                },
            ));
        });
}

fn present(
    session: Res<Session>,
    rounds: Query<Ref<MatchState>>,
    keys: Res<ButtonInput<KeyCode>>,
    players: Query<&PlayerId, With<Predicted>>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut panel: Single<&mut Node, With<Scoreboard>>,
    mut text: Single<&mut Text, With<ScoreboardText>>,
    mut page: Local<usize>,
    mut previous_size: Local<Vec2>,
    mut wheel: MessageReader<MouseWheel>,
) {
    let round = rounds.iter().next();
    let visible = session.is_playing()
        && round
            .as_ref()
            .is_some_and(|r| r.results || keys.pressed(KeyCode::Tab));
    let display = if visible {
        Display::Flex
    } else {
        Display::None
    };
    let opened = panel.display != display;
    if opened {
        panel.display = display;
        *page = 0;
    }
    if !visible {
        wheel.clear();
        return;
    }
    let round = round.unwrap();
    let scroll: f32 = wheel.read().map(|e| e.y).sum();
    let next = keys.just_pressed(KeyCode::PageDown) || scroll < 0.0;
    let prev = keys.just_pressed(KeyCode::PageUp) || scroll > 0.0;
    if next {
        *page += 1;
    }
    if prev {
        *page = page.saturating_sub(1);
    }
    let size = Vec2::new(window.width(), window.height());
    if !opened && !round.is_changed() && !next && !prev && *previous_size == size {
        return;
    }
    *previous_size = size;
    let per_page = (((window.height() * 0.85 - 160.0) / 23.0) as usize).clamp(4, 16);
    let rows = round.ranked();
    let pages = rows.len().div_ceil(per_page).max(1);
    *page = (*page).min(pages - 1);
    let own = players.iter().next().map(|p| p.0);
    let heading = if round.results {
        "RESULTS"
    } else {
        "SCOREBOARD"
    };
    let clock = if round.results {
        format!("Next match in {}s", round.remaining_seconds)
    } else {
        format!(
            "Time left {:02}:{:02}",
            round.remaining_seconds / 60,
            round.remaining_seconds % 60
        )
    };
    let mut label = format!(
        "MATCH {} — {heading}\n{clock}\n\n    #  NICKNAME               KILLS DEATHS\n",
        round.number
    );
    for (rank, row) in rows
        .iter()
        .enumerate()
        .skip(*page * per_page)
        .take(per_page)
    {
        let marker = if Some(row.id) == own { ">" } else { " " };
        let offline = if row.connected { " " } else { "*" };
        label.push_str(&format!(
            "{marker} {offline}{:>2}. {:<20} {:>5} {:>6}\n",
            rank + 1,
            row.nickname,
            row.kills,
            row.deaths
        ));
    }
    if rows.is_empty() {
        label.push_str("No participants yet\n");
    }
    label.push_str("\n> You   * Disconnected");
    if pages > 1 {
        label.push_str(&format!(
            "\nPage {}/{} — PgUp/PgDn or wheel",
            *page + 1,
            pages
        ));
    }
    if !round.results {
        label.push_str("\nHold Tab to view");
    }
    if text.0 != label {
        text.0 = label;
    }
}
