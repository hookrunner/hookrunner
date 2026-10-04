use bevy::prelude::*;
use hookrunner_shared::{
    protocol::LobbyChannel,
    tuning::{EditReply, EditRequest, FeatureSettings},
};
use lightyear::prelude::{server::ClientOf, *};

pub struct TuningPlugin;
impl Plugin for TuningPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FeatureSettings>()
            .add_systems(Startup, setup)
            .add_systems(PreUpdate, edit.after(MessageSystems::Receive))
            .add_systems(PostUpdate, publish.before(ReplicationSystems::Send));
    }
}
fn setup(mut commands: Commands, settings: Res<FeatureSettings>) {
    commands.spawn((settings.clone(), Replicate::to_clients(NetworkTarget::All)));
}
type EditingClients = (With<ClientOf>, With<Connected>);
type EditConnection = (
    &'static mut MessageReceiver<EditRequest>,
    &'static mut MessageSender<EditReply>,
    Has<crate::Joined>,
);
pub(crate) fn edit(
    mut settings: ResMut<FeatureSettings>,
    mut clients: Query<EditConnection, EditingClients>,
) {
    for (mut requests, mut replies, joined) in &mut clients {
        for request in requests.receive() {
            let error = if joined {
                settings.apply(request.revision, &request.edit).err()
            } else {
                Some("Join the match before editing settings.".into())
            };
            replies.send::<LobbyChannel>(EditReply {
                id: request.id,
                settings: settings.clone(),
                error,
            });
        }
    }
}
fn publish(settings: Res<FeatureSettings>, mut snapshot: Single<&mut FeatureSettings>) {
    if **snapshot != *settings {
        **snapshot = settings.clone();
    }
}
