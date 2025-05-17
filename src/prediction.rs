use crate::{
    interpolation::Interpolate, interpolation::SnapshotBuffer, Interpolated, NetworkOwner,
};
use bevy::ecs::component::Mutable;
use bevy::prelude::*;
use bevy::{
    app::{App, Update},
    ecs::{
        component::Component,
        entity::Entity,
        event::{Event, EventReader},
        query::{Added, With, Without},
        resource::Resource,
        system::{Commands, Query, Res, ResMut},
    },
    reflect::Reflect,
    time::Time,
};
use bevy_replicon::client::confirm_history::ConfirmHistory;
use bevy_replicon::prelude::*;
use bevy_replicon::shared::backend::connected_client::NetworkId;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::vec_deque::Iter;
use std::collections::VecDeque;
use std::fmt::Debug;

/// This trait defines how an event will mutate a given component
/// and is required for prediction.
pub trait Predict<E: Event, T>
where
    Self: Component + Interpolate,
{
    fn apply_event(&mut self, event: &E, delta_time: f32, context: &T);
}

pub struct EventSnapshot<T: Event> {
    pub value: T,
    pub tick: u32,
    pub delta_time: f32,
}

#[derive(Resource)]
pub struct PredictedEventHistory<T: Event>(pub VecDeque<EventSnapshot<T>>);

#[derive(Component, Deserialize, Serialize, Reflect, Default)]
pub struct OwnerPredicted;

#[derive(Component, Reflect)]
pub struct Predicted;

impl<T: Event> PredictedEventHistory<T> {
    pub fn new() -> PredictedEventHistory<T> {
        Self(VecDeque::new())
    }
    pub fn insert(&mut self, value: T, tick: u32, delta_time: f32) -> &mut Self {
        self.0.push_back(EventSnapshot {
            value,
            tick,
            delta_time,
        });
        self
    }
    pub fn remove_stale(&mut self, latest_server_snapshot_tick: u32) -> &mut Self {
        if let Some(last_index) = self
            .0
            .iter()
            .position(|v| v.tick >= latest_server_snapshot_tick)
        {
            self.0.drain(0..last_index);
        } else {
            self.0.clear();
        }
        self
    }

    pub fn predict(&mut self, latest_server_snapshot_tick: u32) -> Iter<'_, EventSnapshot<T>> {
        self.remove_stale(latest_server_snapshot_tick);
        self.0.iter()
    }
}

pub fn owner_prediction_init_system(
    trigger: Trigger<OnAdd, OwnerPredicted>,
    q_owners: Query<(Entity, &NetworkId)>,
    // client: Res<RepliconClient>,
    mut commands: Commands,
) {
    for (e, _) in q_owners.iter() {
        if e == trigger.target() {
            commands.entity(e).insert(Predicted);
        } else {
            commands.entity(e).insert(Interpolated);
        }
    }
}

/// Advances the snapshot buffer time for predicted entities.
pub fn predicted_snapshot_system<T: Component + Interpolate + Clone>(
    mut q: Query<&mut SnapshotBuffer<T>, (Without<Interpolated>, With<Predicted>)>,
    time: Res<Time>,
) {
    for mut snapshot_buffer in q.iter_mut() {
        snapshot_buffer.time_since_last_snapshot += time.delta_secs();
    }
}

/// Server implementation
pub fn server_update_system<
    E: Event,
    T: Component,
    C: Component<Mutability=Mutable> + Interpolate + Predict<E, T> + Clone,
>(
    trigger: Trigger<FromClient<E>>,
    time: Res<Time>,
    mut subjects: Query<(&NetworkOwner, &mut C, &T), Without<Predicted>>,
) {
    for (player, mut component, context) in &mut subjects {
        if trigger.client_entity == player.0 {
            println!("Server");
            component.apply_event(trigger.event(), time.delta_secs(), context);
        }
    }
}

// Client prediction implementation
pub fn predicted_update_system<
    E: Event + Clone,
    T: Component,
    C: Component<Mutability=Mutable> + Interpolate + Predict<E, T> + Clone,
>(
    local_events: Trigger<FromClient<E>>,
    mut q_predicted_players: Query<
        (&mut C, &SnapshotBuffer<C>, &ConfirmHistory, &T),
        (With<Predicted>, Without<Interpolated>),
    >,
    mut event_history: ResMut<PredictedEventHistory<E>>,
    time: Res<Time>,
) {
    // Apply all pending inputs to latest snapshot
    for (mut component, snapshot_buffer, confirmed, context) in q_predicted_players.iter_mut() {
        // Append the latest input event
        event_history.insert(
            local_events.event.clone(),
            confirmed.last_tick().get(),
            time.delta_secs(),
        );


        let mut corrected_component = snapshot_buffer.latest_snapshot();
        for event_snapshot in event_history.predict(snapshot_buffer.latest_snapshot_tick()) {
            corrected_component.apply_event(
                &event_snapshot.value,
                event_snapshot.delta_time,
                context,
            );
        }
        println!("C");
        *component = corrected_component;
    }
}

pub trait AppPredictionExt {
    /// Register an event for client-side prediction, this will make sure a history of past events
    /// is stored for the client to be able to replay them in case of a server correction
    fn add_client_predicted_event<E>(&mut self, channel: Channel) -> &mut Self
    where
        E: Event + Serialize + DeserializeOwned + Debug + Clone;

    /// Register a component and event pair for prediction.
    /// This will generate serverside and clientside systems that use the implementation from the
    /// `Predict` trait to allow prediction and serverside correction
    fn predict_event_for_component<E, T, C>(&mut self) -> &mut Self
    where
        E: Event + Serialize + DeserializeOwned + Debug + Clone,
        T: Component<Mutability=Mutable> + Serialize + DeserializeOwned,
        C: Component<Mutability=Mutable> + Predict<E, T> + Clone;
}

impl AppPredictionExt for App {
    fn add_client_predicted_event<E>(&mut self, channel: Channel) -> &mut Self
    where
        E: Event + Serialize + DeserializeOwned + Debug + Clone,
    {
        let history: PredictedEventHistory<E> = PredictedEventHistory::new();
        self.insert_resource(history);
        self.add_client_trigger::<E>(channel)
    }

    fn predict_event_for_component<E, T, C>(&mut self) -> &mut Self
    where
        E: Event + Serialize + DeserializeOwned + Debug + Clone,
        T: Component<Mutability=Mutable> + Serialize + DeserializeOwned,
        C: Component<Mutability=Mutable> + Predict<E, T> + Clone,
    {
        self.add_observer(
            predicted_update_system::<E, T, C>
        )
            .add_observer(server_update_system::<E, T, C>)
            .replicate::<T>()
    }
}
