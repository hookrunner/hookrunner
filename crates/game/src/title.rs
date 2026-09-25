//! The title screen and nickname editor are shared by browser and native clients.
use bevy::{
    input::keyboard::{Key, KeyboardInput},
    prelude::*,
};
use hookrunner_client::{JoinGame, Session, SessionPhase};
use hookrunner_shared::nickname::{self, MAX_NICKNAME_CHARS};

pub struct TitlePlugin;
impl Plugin for TitlePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NicknameDraft>()
            .add_systems(Startup, setup)
            .add_systems(
                Update,
                (edit, present)
                    .chain()
                    .after(hookrunner_client::update_session),
            );
    }
}

#[derive(Resource, Default)]
struct NicknameDraft {
    value: String,
    error: Option<String>,
    selected: bool,
    composing: bool,
}
#[derive(Component)]
struct TitleScreen;
#[derive(Component)]
struct NicknameText;
#[derive(Component)]
struct StatusText;
#[derive(Component)]
struct PlayButton;
#[derive(Component)]
struct PlayLabel;

fn setup(mut commands: Commands) {
    commands
        .spawn((
            TitleScreen,
            GlobalZIndex(900),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                padding: UiRect::all(px(24)),
                ..default()
            },
            BackgroundColor(Color::srgb(0.035, 0.045, 0.06)),
        ))
        .with_children(|root| {
            root.spawn(Node {
                width: px(420),
                max_width: percent(100),
                flex_direction: FlexDirection::Column,
                row_gap: px(18),
                ..default()
            })
            .with_children(|panel| {
                panel.spawn((
                    Text::new("HOOKRUNNER"),
                    TextFont {
                        font_size: 42.0,
                        ..default()
                    },
                ));
                panel.spawn((
                    Text::new("Choose your nickname"),
                    TextFont {
                        font_size: 20.0,
                        ..default()
                    },
                ));
                panel
                    .spawn((
                        Node {
                            min_height: px(58),
                            padding: UiRect::all(px(16)),
                            border: UiRect::all(px(2)),
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.09, 0.12, 0.16)),
                        BorderColor::all(Color::srgb(0.3, 0.75, 0.8)),
                    ))
                    .with_children(|field| {
                        field.spawn((
                            NicknameText,
                            Text::new("Type here…"),
                            TextFont {
                                font_size: 22.0,
                                ..default()
                            },
                        ));
                    });
                panel.spawn((
                    Text::new("1–20 characters · Enter to play\nLetters, numbers, spaces, _ and -"),
                    TextFont {
                        font_size: 15.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.6, 0.68, 0.75)),
                ));
                panel
                    .spawn((
                        Button,
                        PlayButton,
                        Node {
                            min_height: px(54),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.12, 0.4, 0.46)),
                    ))
                    .with_children(|button| {
                        button.spawn((
                            PlayLabel,
                            Text::new("Play"),
                            TextFont {
                                font_size: 22.0,
                                ..default()
                            },
                        ));
                    });
                panel.spawn((
                    StatusText,
                    Text::new(""),
                    TextFont {
                        font_size: 16.0,
                        ..default()
                    },
                    TextColor(Color::srgb(1.0, 0.65, 0.5)),
                ));
            });
        });
}

fn edit(
    mut commands: Commands,
    mut draft: ResMut<NicknameDraft>,
    mut session: ResMut<Session>,
    loading: Res<crate::loading::LoadingProgress>,
    mut keyboard: MessageReader<KeyboardInput>,
    mut ime: MessageReader<Ime>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Query<&Interaction, (Changed<Interaction>, With<PlayButton>)>,
) {
    if session.phase != SessionPhase::Title || loading.percent != 100 {
        keyboard.clear();
        ime.clear();
        return;
    }
    let mut committed = false;
    for event in ime.read() {
        match event {
            Ime::Preedit { value, .. } => draft.composing = !value.is_empty(),
            Ime::Commit { value, .. } => {
                insert_text(&mut draft, value);
                draft.composing = false;
                committed = true;
            }
            Ime::Disabled { .. } => draft.composing = false,
            _ => (),
        }
    }
    let modifier = keys.any_pressed([
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
    ]);
    let mut submit = buttons
        .iter()
        .any(|interaction| *interaction == Interaction::Pressed);
    for event in keyboard.read() {
        if !event.state.is_pressed() || draft.composing {
            continue;
        }
        match &event.logical_key {
            Key::Enter if !event.repeat && !committed => submit = true,
            Key::Backspace => {
                if draft.selected {
                    draft.value.clear();
                    draft.selected = false;
                } else {
                    draft.value.pop();
                }
                draft.error = None;
                session.error = None;
            }
            Key::Character(value) if modifier && value.eq_ignore_ascii_case("a") => {
                draft.selected = true
            }
            _ if !modifier && !committed => {
                if let Some(text) = &event.text {
                    insert_text(&mut draft, text);
                    session.error = None;
                }
            }
            _ => (),
        }
    }
    if submit {
        match nickname::validate(&draft.value) {
            Ok(nickname) => {
                draft.value = nickname.clone();
                draft.error = None;
                commands.trigger(JoinGame { nickname });
            }
            Err(error) => draft.error = Some(error.into()),
        }
    }
}

fn insert_text(draft: &mut NicknameDraft, text: &str) {
    if text.chars().any(char::is_control) {
        return;
    }
    if draft.selected {
        draft.value.clear();
        draft.selected = false;
    }
    let remaining = MAX_NICKNAME_CHARS.saturating_sub(draft.value.chars().count());
    draft.value.extend(text.chars().take(remaining));
    draft.error = None;
}

fn present(
    session: Res<Session>,
    draft: Res<NicknameDraft>,
    mut screen: Single<&mut Node, With<TitleScreen>>,
    mut texts: Query<(
        &mut Text,
        Has<NicknameText>,
        Has<StatusText>,
        Has<PlayLabel>,
    )>,
    mut windows: Query<&mut Window>,
    mut previous_playing: Local<Option<bool>>,
) {
    if !session.is_changed() && !draft.is_changed() && previous_playing.is_some() {
        return;
    }
    let playing = session.is_playing();
    screen.display = if playing {
        Display::None
    } else {
        Display::Flex
    };
    if *previous_playing != Some(playing) {
        crate::platform::set_playing(playing);
        for mut window in &mut windows {
            window.ime_enabled = !playing;
        }
        *previous_playing = Some(playing);
    }
    for (mut text, nickname, status, label) in &mut texts {
        if nickname {
            text.0 = if draft.value.is_empty() {
                "Type here…".into()
            } else if draft.selected {
                format!("[{}]", draft.value)
            } else {
                format!("{}|", draft.value)
            };
        } else if status {
            text.0 = draft
                .error
                .as_ref()
                .or(session.error.as_ref())
                .cloned()
                .unwrap_or_default();
        } else if label {
            text.0 = if session.phase == SessionPhase::Connecting {
                "Connecting…"
            } else {
                "Play"
            }
            .into();
        }
    }
}
