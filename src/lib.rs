use std::fmt::Debug;

use bevy::prelude::*;
use bevy_replicon::prelude::*;
use bevy_replicon_renet::renet::ClientId;
use serde::{Deserialize, Serialize};

pub use bevy_replicon_snap_macros;

use crate::{
    interpolation::{Interpolated, SnapshotInterpolationConfig},
    prediction::{owner_prediction_init_system, OwnerPredicted, Predicted},
};

pub mod interpolation;
pub mod prediction;

pub struct SnapshotInterpolationPlugin {
    /// Should reflect the server max tick rate
    pub max_tick_rate: u16,
}

#[derive(Component, Deserialize, Serialize, Reflect, Deref)]
pub struct NetworkOwner(pub Entity);

#[derive(Component, Deserialize, Serialize, Reflect, Deref)]
pub struct ClientNetId(pub ClientId);

/// Sets for interpolation systems.
#[derive(SystemSet, Debug, Hash, PartialEq, Eq, Clone, Copy)]
pub enum InterpolationSet {
    /// Systems that initializes buffers and flag components for replicated entities.
    ///
    /// Runs in `PreUpdate`.
    Init,
    /// Systems that calculating interpolation.
    ///
    /// Runs in `PreUpdate`.
    Interpolate,
}

impl Plugin for SnapshotInterpolationPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<Interpolated>()
            .register_type::<OwnerPredicted>()
            .register_type::<NetworkOwner>()
            .register_type::<Predicted>()
            .replicate::<Interpolated>()
            .replicate::<NetworkOwner>()
            .replicate::<OwnerPredicted>()
            .configure_sets(PreUpdate, InterpolationSet::Init.after(ClientSet::Receive))
            .configure_sets(
                PreUpdate,
                InterpolationSet::Interpolate.after(InterpolationSet::Init),
            )
            .add_observer(
                owner_prediction_init_system
            )
            .insert_resource(SnapshotInterpolationConfig {
                max_tick_rate: self.max_tick_rate,
            });
    }
}
