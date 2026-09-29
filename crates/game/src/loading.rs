//! Shared loading state and UI. Platform code only presents this state.
use bevy::prelude::*;

#[derive(Resource)]
pub struct LoadingProgress {
    pub percent: u8,
    pub error: Option<String>,
}

impl Default for LoadingProgress {
    fn default() -> Self {
        // The runtime is running; browser download/initialization is complete.
        Self {
            percent: 20,
            error: None,
        }
    }
}

impl LoadingProgress {
    pub fn advance(&mut self, percent: u8) {
        self.percent = self.percent.max(percent.min(100));
    }
}

pub struct LoadingPlugin;

impl Plugin for LoadingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LoadingProgress>()
            .add_systems(Startup, setup)
            .add_systems(Update, present.after(crate::map::finish_loading));
    }
}

#[derive(Component)]
struct LoadingScreen;
#[derive(Component)]
struct LoadingLabel;

fn setup(mut commands: Commands) {
    commands
        .spawn((
            LoadingScreen,
            GlobalZIndex(1000),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                padding: UiRect::all(px(32)),
                ..default()
            },
            BackgroundColor(Color::srgb(0.067, 0.067, 0.067)),
        ))
        .with_children(|parent| {
            parent.spawn((
                LoadingLabel,
                Text::new("Loading 20%"),
                TextFont {
                    font_size: 20.0,
                    ..default()
                },
                TextColor(Color::srgb(0.78, 0.78, 0.78)),
            ));
        });
}

fn present(
    mut commands: Commands,
    progress: Res<LoadingProgress>,
    screen: Query<Entity, With<LoadingScreen>>,
    mut labels: Query<&mut Text, With<LoadingLabel>>,
) {
    if !progress.is_changed() {
        return;
    }
    for mut text in &mut labels {
        text.0 = match &progress.error {
            Some(error) => format!(
                "Loading failed at {}%\n{error}\nPlease restart the game.",
                progress.percent
            ),
            None => format!("Loading {}%", progress.percent),
        };
    }
    crate::platform::loading_progress(progress.percent, progress.error.as_deref());
    if progress.percent == 100 && progress.error.is_none() {
        for entity in &screen {
            commands.entity(entity).despawn();
        }
        crate::platform::finished_loading();
    }
}
