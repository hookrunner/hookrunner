use bevy::prelude::*;
use hookrunner_shared::{
    protocol::LobbyChannel,
    tuning::{Edit, EditReply, EditRequest, FeatureSettings},
};
use lightyear::prelude::*;
use std::collections::VecDeque;

#[derive(Resource, Default)]
pub struct TuningClient {
    pub settings: Option<FeatureSettings>,
    pub error: Option<String>,
    queue: VecDeque<Edit>,
    pending: Option<u64>,
    next_id: u64,
}
impl TuningClient {
    pub fn values(&self) -> hookrunner_shared::tuning::SettingsValues {
        self.settings
            .as_ref()
            .map_or_else(Default::default, |s| s.values)
    }
    pub fn queue(&mut self, edit: Edit) {
        if self.queue.len() < 32 {
            self.error = None;
            self.queue.push_back(edit);
        } else {
            self.error = Some("Wait for the current edits to finish.".into());
        }
    }
    pub fn busy(&self) -> bool {
        self.pending.is_some() || !self.queue.is_empty()
    }
}

pub(crate) struct TuningPlugin;
impl Plugin for TuningPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TuningClient>()
            .add_systems(Update, sync.after(crate::update_session));
    }
}

type SettingsConnection = (With<Client>, With<Connected>);
pub fn sync(
    session: Res<crate::Session>,
    snapshots: Query<&FeatureSettings>,
    mut state: ResMut<TuningClient>,
    mut connections: Query<
        (
            &mut MessageSender<EditRequest>,
            &mut MessageReceiver<EditReply>,
        ),
        SettingsConnection,
    >,
) {
    if !session.lobby_ready {
        *state = TuningClient::default();
        return;
    }
    if let Ok(snapshot) = snapshots.single()
        && state
            .settings
            .as_ref()
            .is_none_or(|current| snapshot.revision >= current.revision)
        && state.settings.as_ref() != Some(snapshot)
    {
        state.settings = Some(snapshot.clone());
    }
    let Ok((mut sender, mut replies)) = connections.single_mut() else {
        return;
    };
    for reply in replies.receive() {
        if state
            .settings
            .as_ref()
            .is_none_or(|current| reply.settings.revision >= current.revision)
        {
            state.settings = Some(reply.settings);
        }
        if state.pending == Some(reply.id) {
            state.pending = None;
            state.error = reply.error;
            if state.error.is_some() {
                state.queue.clear();
            }
        }
    }
    if !session.is_playing() {
        state.queue.clear();
        return;
    }
    if state.pending.is_none()
        && let Some(settings) = &state.settings
    {
        let revision = settings.revision;
        if let Some(edit) = state.queue.pop_front() {
            state.next_id += 1;
            let id = state.next_id;
            sender.send::<LobbyChannel>(EditRequest { id, revision, edit });
            state.pending = Some(id);
        }
    }
}
