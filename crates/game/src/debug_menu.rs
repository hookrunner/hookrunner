use bevy::{
    color::Mix,
    ecs::system::SystemParam,
    input::{
        keyboard::{Key, KeyboardInput},
        mouse::{MouseScrollUnit, MouseWheel},
    },
    prelude::*,
    window::PrimaryWindow,
};
use hookrunner_client::{Session, tuning::TuningClient};
use hookrunner_shared::tuning::{Category, Edit, PARAMETERS, Parameter};

#[derive(Resource, Default)]
pub(crate) struct DebugMenu {
    pub open: bool,
    folder: Option<Category>,
    favorites: Vec<Parameter>,
    focused: Option<Parameter>,
    draft: String,
    cursor: usize,
    selected: bool,
    help: Option<Parameter>,
    error: Option<String>,
}

impl DebugMenu {
    fn commit(&mut self, client: &mut TuningClient) -> bool {
        let Some(parameter) = self.focused else {
            return true;
        };
        let result = parameter.spec().kind.parse(&self.draft).and_then(|value| {
            parameter.spec().validate(value)?;
            if client
                .settings
                .as_ref()
                .is_some_and(|settings| settings.values.get(parameter) != value)
            {
                client.queue(Edit::Set(parameter, value));
            }
            Ok(())
        });
        match result {
            Ok(()) => {
                self.focused = None;
                self.error = None;
                true
            }
            Err(error) => {
                self.error = Some(error);
                false
            }
        }
    }
    fn focus(&mut self, parameter: Parameter, client: &TuningClient) {
        self.focused = Some(parameter);
        self.draft = client.settings.as_ref().map_or_else(
            || parameter.spec().default.to_string(),
            |settings| settings.values.get(parameter).to_string(),
        );
        self.cursor = self.draft.len();
        self.selected = true;
        self.error = None;
    }
    fn insert(&mut self, text: &str) {
        if self.selected {
            self.draft.clear();
            self.cursor = 0;
            self.selected = false;
        }
        for c in text
            .chars()
            .filter(|c| c.is_ascii() && !c.is_ascii_control())
        {
            if self.draft.len() < 64 {
                self.draft.insert(self.cursor, c);
                self.cursor += 1;
            }
        }
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct MenuInput;
pub struct DebugMenuPlugin;
impl Plugin for DebugMenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DebugMenu>()
            .add_systems(Startup, setup)
            .add_systems(
                PreUpdate,
                hotkeys.in_set(MenuInput).after(bevy::input::InputSystems),
            )
            .add_systems(
                Update,
                (interact, edit_keys, present, highlight, fit, scroll)
                    .chain()
                    .after(hookrunner_client::tuning::sync)
                    .before(crate::view::CameraUpdated),
            );
    }
}

#[derive(Component)]
struct MenuRoot;
#[derive(Component)]
struct Panel;
#[derive(Component)]
struct PathBar;
#[derive(Component)]
struct Listing;
#[derive(Component)]
struct ValueLabel(Parameter);
#[derive(Component)]
struct ButtonShade(f32);
#[derive(Component)]
struct Notice;
#[derive(Component)]
struct Help;
#[derive(Component)]
struct HelpPanel;
#[derive(Component, Clone, Copy)]
pub(crate) enum Action {
    Open(Option<Category>),
    Edit(Parameter),
    Help(Parameter),
    Favorite(Parameter),
    ResetParameter(Parameter),
    ResetCategory(Category),
    ResetAll,
    Close,
}

fn menu_font() -> TextFont {
    TextFont {
        font_size: 14.0,
        ..default()
    }
}
fn text(parent: &mut ChildSpawnerCommands, value: &str) {
    parent.spawn((
        Text::new(value),
        menu_font(),
        TextColor(Color::srgb(0.82, 0.82, 0.82)),
    ));
}
fn button(parent: &mut ChildSpawnerCommands, action: Action, label: &str, width: Val) {
    parent
        .spawn((
            Button,
            action,
            Node {
                width,
                min_height: px(32),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                justify_content: if matches!(action, Action::Open(_)) {
                    JustifyContent::Start
                } else {
                    JustifyContent::Center
                },
                padding: UiRect::horizontal(px(8)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            ButtonShade(0.10),
            BackgroundColor(Color::srgb(0.10, 0.10, 0.10)),
        ))
        .with_children(|button| text(button, label));
}
fn row_node() -> Node {
    Node {
        flex_direction: FlexDirection::Row,
        flex_wrap: FlexWrap::Wrap,
        align_items: AlignItems::Center,
        column_gap: px(6),
        row_gap: px(4),
        padding: UiRect::axes(px(8), px(2)),
        flex_shrink: 0.0,
        ..default()
    }
}
fn parameter_row(parent: &mut ChildSpawnerCommands, parameter: Parameter, favorite: bool) {
    parent
        .spawn((row_node(), BackgroundColor(Color::NONE)))
        .with_children(|row| {
            row.spawn(Node {
                flex_grow: 1.0,
                min_width: px(150),
                padding: UiRect::horizontal(px(8)),
                ..default()
            })
            .with_children(|name| text(name, parameter.spec().name));
            row.spawn((
                Button,
                Action::Edit(parameter),
                Node {
                    width: px(132),
                    overflow: Overflow::clip(),
                    min_height: px(32),
                    padding: UiRect::all(px(6)),
                    align_items: AlignItems::Center,
                    border_radius: BorderRadius::all(px(6)),
                    flex_shrink: 0.0,
                    ..default()
                },
                ButtonShade(0.075),
                BackgroundColor(Color::srgb(0.075, 0.075, 0.075)),
            ))
            .with_children(|field| {
                field.spawn((
                    ValueLabel(parameter),
                    TextColor(Color::srgb(0.88, 0.88, 0.88)),
                    TextBackgroundColor(Color::NONE),
                    Text::new(""),
                    menu_font(),
                ));
            });
            button(row, Action::Help(parameter), "?", px(32));
            button(
                row,
                Action::Favorite(parameter),
                if favorite { "★" } else { "☆" },
                px(32),
            );
            button(row, Action::ResetParameter(parameter), "↺", px(32));
        });
}
fn folder_row(parent: &mut ChildSpawnerCommands, category: Category) {
    parent.spawn(row_node()).with_children(|row| {
        row.spawn(Node {
            flex_grow: 1.0,
            min_width: px(0),
            ..default()
        })
        .with_children(|folder| {
            button(
                folder,
                Action::Open(Some(category)),
                category.name(),
                percent(100),
            )
        });
        button(row, Action::ResetCategory(category), "↺", px(32));
    });
}

fn setup(mut commands: Commands) {
    commands
        .spawn((
            MenuRoot,
            GlobalZIndex(1000),
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.48)),
        ))
        .with_children(|root| {
            root.spawn((
                Panel,
                Node {
                    width: px(600),
                    height: px(600),
                    padding: UiRect::all(px(16)),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(12),
                    border_radius: BorderRadius::all(px(8)),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.10, 0.10, 0.10)),
            ))
            .with_children(|panel| {
                panel
                    .spawn(Node {
                        flex_direction: FlexDirection::Row,
                        column_gap: px(6),
                        row_gap: px(6),
                        flex_wrap: FlexWrap::Wrap,
                        align_items: AlignItems::Center,
                        flex_shrink: 0.0,
                        ..default()
                    })
                    .with_children(|top| {
                        top.spawn((
                            PathBar,
                            Node {
                                flex_grow: 1.0,
                                flex_direction: FlexDirection::Row,
                                flex_wrap: FlexWrap::Wrap,
                                align_items: AlignItems::Center,
                                column_gap: px(4),
                                row_gap: px(4),
                                ..default()
                            },
                        ));
                        button(top, Action::ResetAll, "↺ all", px(64));
                        button(top, Action::Close, "×", px(32));
                    });
                panel.spawn((
                    Listing,
                    ScrollPosition::default(),
                    Node {
                        flex_grow: 1.0,
                        flex_shrink: 1.0,
                        min_height: px(0),
                        flex_direction: FlexDirection::Column,
                        overflow: Overflow::scroll_y(),
                        row_gap: px(2),
                        ..default()
                    },
                ));
                panel
                    .spawn((
                        HelpPanel,
                        Interaction::default(),
                        ScrollPosition::default(),
                        Node {
                            display: Display::None,
                            max_height: percent(45),
                            min_height: px(0),
                            overflow: Overflow::scroll_y(),
                            flex_shrink: 0.0,
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.075, 0.075, 0.075)),
                    ))
                    .with_child((
                        Help,
                        Text::new(""),
                        menu_font(),
                        Node {
                            width: percent(100),
                            min_width: px(0),
                            padding: UiRect::all(px(10)),
                            flex_shrink: 0.0,
                            ..default()
                        },
                    ));
                panel.spawn((
                    Notice,
                    Text::new(""),
                    menu_font(),
                    TextColor(Color::srgb(0.9, 0.68, 0.68)),
                    Node {
                        display: Display::None,
                        flex_shrink: 0.0,
                        ..default()
                    },
                ));
            });
        });
}

fn hotkeys(keys: Res<ButtonInput<KeyCode>>, session: Res<Session>, mut menu: ResMut<DebugMenu>) {
    let previous = menu.open;
    if !session.is_playing() {
        menu.open = false;
    } else if keys.just_pressed(KeyCode::F2) {
        menu.open = !menu.open;
    } else if menu.open && keys.just_pressed(KeyCode::Escape) {
        menu.open = false;
    }
    if previous != menu.open {
        menu.focused = None;
        menu.error = None;
        menu.help = None;
        crate::platform::set_debug_menu(menu.open);
    }
}
type ChangedButtons = (Changed<Interaction>, With<Button>);
fn interact(
    mut menu: ResMut<DebugMenu>,
    mut client: ResMut<TuningClient>,
    buttons: Query<(&Interaction, &Action), ChangedButtons>,
) {
    if !menu.open {
        return;
    }
    for (interaction, action) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match *action {
            Action::Open(folder) => {
                if menu.commit(&mut client) {
                    menu.folder = folder;
                    menu.help = None;
                }
            }
            Action::Edit(parameter) => {
                if menu.focused == Some(parameter) {
                    menu.selected = true;
                } else if menu.commit(&mut client) {
                    menu.focus(parameter, &client);
                }
            }
            Action::Help(parameter) => {
                menu.help = if menu.help == Some(parameter) {
                    None
                } else {
                    Some(parameter)
                };
            }
            Action::Favorite(parameter) => {
                if let Some(index) = menu.favorites.iter().position(|p| *p == parameter) {
                    // Commit before removing the field currently being edited from root.
                    if menu.folder.is_none()
                        && menu.focused == Some(parameter)
                        && !menu.commit(&mut client)
                    {
                        continue;
                    }
                    menu.favorites.remove(index);
                    if menu.folder.is_none() && menu.help == Some(parameter) {
                        menu.help = None;
                    }
                } else {
                    menu.favorites.push(parameter);
                }
            }
            Action::ResetParameter(parameter) => {
                if menu.focused == Some(parameter) {
                    menu.focused = None;
                }
                client.queue(Edit::ResetParameter(parameter));
                menu.error = None;
            }
            Action::ResetCategory(category) => {
                if menu
                    .focused
                    .is_some_and(|p| p.spec().category.is_within(category))
                {
                    menu.focused = None;
                }
                client.queue(Edit::ResetCategory(category));
                menu.error = None;
            }
            Action::ResetAll => {
                menu.focused = None;
                menu.error = None;
                client.queue(Edit::ResetAll);
            }
            Action::Close => {
                if menu.commit(&mut client) {
                    menu.open = false;
                    crate::platform::set_debug_menu(false);
                }
            }
        }
    }
}

fn edit_keys(
    mut menu: ResMut<DebugMenu>,
    mut client: ResMut<TuningClient>,
    keys: Res<ButtonInput<KeyCode>>,
    mut events: MessageReader<KeyboardInput>,
) {
    if !menu.open || menu.focused.is_none() {
        events.clear();
        return;
    }
    let modifier = keys.any_pressed([
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
    ]);
    for event in events.read().filter(|event| event.state.is_pressed()) {
        if menu.focused.is_none() {
            continue;
        }
        match &event.logical_key {
            Key::Enter => {
                menu.commit(&mut client);
            }
            Key::Character(value) if modifier && value.eq_ignore_ascii_case("a") => {
                menu.selected = true
            }
            Key::ArrowLeft => {
                menu.cursor = if menu.selected {
                    0
                } else {
                    menu.cursor.saturating_sub(1)
                };
                menu.selected = false;
            }
            Key::ArrowRight => {
                menu.cursor = if menu.selected {
                    menu.draft.len()
                } else {
                    (menu.cursor + 1).min(menu.draft.len())
                };
                menu.selected = false;
            }
            Key::Home => {
                menu.cursor = 0;
                menu.selected = false;
            }
            Key::End => {
                menu.cursor = menu.draft.len();
                menu.selected = false;
            }
            Key::Backspace | Key::Delete => {
                if menu.selected {
                    menu.draft.clear();
                    menu.cursor = 0;
                    menu.selected = false;
                } else if event.logical_key == Key::Backspace && menu.cursor > 0 {
                    menu.cursor -= 1;
                    let cursor = menu.cursor;
                    menu.draft.remove(cursor);
                } else if event.logical_key == Key::Delete && menu.cursor < menu.draft.len() {
                    let cursor = menu.cursor;
                    menu.draft.remove(cursor);
                }
                menu.error = None;
            }
            _ if !modifier => {
                if let Some(text) = &event.text {
                    menu.insert(text);
                    menu.error = None;
                }
            }
            _ => (),
        }
    }
}

type PathNodes = Or<(With<PathBar>, With<Listing>)>;
type InfoNodes = (
    Or<(With<Help>, With<Notice>)>,
    Without<MenuRoot>,
    Without<HelpPanel>,
);
type ValueNodes = (Without<Help>, Without<Notice>);
#[derive(SystemParam)]
struct MenuUi<'w, 's> {
    roots: Query<'w, 's, (Entity, Has<PathBar>), PathNodes>,
    root: Single<'w, 's, &'static mut Node, With<MenuRoot>>,
    values: Query<
        'w,
        's,
        (
            &'static ValueLabel,
            &'static mut Text,
            &'static mut TextBackgroundColor,
        ),
        ValueNodes,
    >,
    help: Single<
        'w,
        's,
        (&'static mut Node, &'static mut ScrollPosition),
        (
            With<HelpPanel>,
            Without<MenuRoot>,
            Without<Help>,
            Without<Notice>,
        ),
    >,
    info: Query<'w, 's, (&'static mut Text, &'static mut Node, Has<Help>), InfoNodes>,
}
fn present(
    mut commands: Commands,
    menu: Res<DebugMenu>,
    client: Res<TuningClient>,
    mut previous_listing: Local<Option<(Option<Category>, Vec<Parameter>)>>,
    mut previous_help: Local<Option<Parameter>>,
    mut ui: MenuUi,
    time: Res<Time>,
) {
    ui.root.display = if menu.open {
        Display::Flex
    } else {
        Display::None
    };
    if !menu.open {
        return;
    }
    ui.help.0.display = if menu.help.is_some() {
        Display::Flex
    } else {
        Display::None
    };
    if *previous_help != menu.help {
        *ui.help.1 = ScrollPosition::default();
        *previous_help = menu.help;
    }
    let listing = (menu.folder, menu.favorites.clone());
    if previous_listing.as_ref() != Some(&listing) {
        let navigated = previous_listing
            .as_ref()
            .is_none_or(|previous| previous.0 != menu.folder);
        for (entity, path) in &ui.roots {
            if path && !navigated {
                continue;
            }
            commands.entity(entity).despawn_related::<Children>();
            commands.entity(entity).with_children(|parent| {
                if path {
                    button(parent, Action::Open(None), "root", Val::Auto);
                    if let Some(folder) = menu.folder {
                        for category in folder.path() {
                            text(parent, "/");
                            button(
                                parent,
                                Action::Open(Some(category)),
                                category.name(),
                                Val::Auto,
                            );
                        }
                    }
                } else {
                    if menu.folder.is_none() {
                        for parameter in &menu.favorites {
                            parameter_row(parent, *parameter, true);
                        }
                    }
                    for category in Category::ALL
                        .iter()
                        .copied()
                        .filter(|c| c.parent() == menu.folder)
                    {
                        folder_row(parent, category);
                    }
                    for spec in PARAMETERS
                        .iter()
                        .filter(|spec| Some(spec.category) == menu.folder)
                    {
                        parameter_row(parent, spec.id, menu.favorites.contains(&spec.id));
                    }
                }
            });
            if !path && navigated {
                commands.entity(entity).insert(ScrollPosition::default());
            }
        }
        *previous_listing = Some(listing);
    }
    for (label, mut text, mut selection) in &mut ui.values {
        selection.0 = if menu.focused == Some(label.0) && menu.selected {
            Color::srgb(0.34, 0.34, 0.34)
        } else {
            Color::NONE
        };
        text.0 = if menu.focused == Some(label.0) {
            if menu.selected {
                menu.draft.clone()
            } else {
                format!(
                    "{}{}{}",
                    &menu.draft[..menu.cursor],
                    if time.elapsed_secs() % 1.0 < 0.5 {
                        "│"
                    } else {
                        ""
                    },
                    &menu.draft[menu.cursor..]
                )
            }
        } else {
            client.settings.as_ref().map_or_else(
                || "…".into(),
                |settings| settings.values.get(label.0).to_string(),
            )
        };
    }
    for (mut text, mut node, help) in &mut ui.info {
        text.0 = if help {
            menu.help
                .map(|parameter| {
                    let spec = parameter.spec();
                    format!(
                        "{}\n{}\n{} · {:?} · {}–{} · Default: {}",
                        spec.name,
                        spec.help,
                        spec.units,
                        spec.kind,
                        spec.min,
                        spec.max,
                        spec.default
                    )
                })
                .unwrap_or_default()
        } else {
            menu.error
                .as_ref()
                .or(client.error.as_ref())
                .cloned()
                .unwrap_or_else(|| {
                    if client.busy() {
                        "Applying…".into()
                    } else {
                        String::new()
                    }
                })
        };
        node.display = if text.0.is_empty() {
            Display::None
        } else {
            Display::Flex
        };
    }
}

fn highlight(
    menu: Res<DebugMenu>,
    time: Res<Time>,
    mut buttons: Query<(&Interaction, &Action, &ButtonShade, &mut BackgroundColor)>,
) {
    if !menu.open {
        return;
    }
    let blend = 1.0 - (-18.0 * time.delta_secs()).exp();
    for (interaction, action, shade, mut background) in &mut buttons {
        let selected = match *action {
            Action::Edit(parameter) => menu.focused == Some(parameter),
            Action::Help(parameter) => menu.help == Some(parameter),
            Action::Favorite(parameter) => menu.favorites.contains(&parameter),
            Action::Open(folder) => menu.folder == folder,
            _ => false,
        };
        let value = match interaction {
            Interaction::Pressed => 0.25,
            Interaction::Hovered if selected => 0.25,
            _ if selected => 0.21,
            Interaction::Hovered => 0.17,
            Interaction::None => shade.0,
        };
        background.0 = background.0.mix(&Color::srgb(value, value, value), blend);
    }
}

fn fit(window: Single<&Window, With<PrimaryWindow>>, mut panel: Single<&mut Node, With<Panel>>) {
    let side = (window.width().min(window.height()) - 24.0).clamp(100.0, 600.0);
    panel.width = px(side);
    panel.height = px(side);
}
fn scroll(
    menu: Res<DebugMenu>,
    mut events: MessageReader<MouseWheel>,
    mut listing: Single<(&ComputedNode, &mut ScrollPosition), (With<Listing>, Without<HelpPanel>)>,
    mut help: Single<
        (&ComputedNode, &Interaction, &mut ScrollPosition),
        (With<HelpPanel>, Without<Listing>),
    >,
) {
    if !menu.open {
        events.clear();
        return;
    }
    for event in events.read() {
        let scale = if event.unit == MouseScrollUnit::Line {
            36.0
        } else {
            1.0
        };
        let (node, position) = if menu.help.is_some() && *help.1 != Interaction::None {
            (help.0, &mut *help.2)
        } else {
            (listing.0, &mut *listing.1)
        };
        let max = ((node.content_size().y - node.size().y) * node.inverse_scale_factor()).max(0.0);
        position.y = (position.y - event.y * scale).clamp(0.0, max);
    }
}
