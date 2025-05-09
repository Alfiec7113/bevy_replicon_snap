//! This is the "Simple Box" example from the bevy_replicon repo with owner predicted players
//! This means the local player is predicted and other networked entities are interpolated

use core::f32;
use std::{
    error::Error,
    net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
    time::SystemTime,
};

use bevy::{prelude::*, winit::UpdateMode::Continuous, winit::WinitSettings};
use bevy_replicon::prelude::*;
use bevy_replicon_renet::{
    netcode::{
        ClientAuthentication, NetcodeClientTransport, NetcodeServerTransport, ServerAuthentication,
        ServerConfig,
    },
    renet::{ConnectionConfig, RenetClient, RenetServer},
    RenetChannelsExt, RepliconRenetPlugins,
};
use bevy_replicon_snap::{
    interpolation::AppInterpolationExt,
    prediction::OwnerPredicted,
    prediction::{AppPredictionExt, Predict},
    NetworkOwner, SnapshotInterpolationPlugin,
};
use bevy_replicon_snap_macros::Interpolate;
use clap::Parser;
use serde::{Deserialize, Serialize};

// Setting an overly low server tickrate to make the difference between the different methods clearly visible
// Usually you would want a server for a realtime game to run with at least 30 ticks per second
const MAX_TICK_RATE: u16 = 30;

fn main() {
    App::new()
        .init_resource::<Cli>() // Parse CLI before creating window.
        // Makes the server/client update continuously even while unfocused.
        .insert_resource(WinitSettings {
            focused_mode: Continuous,
            unfocused_mode: Continuous,
        })
        .add_plugins((
            DefaultPlugins,
            RepliconPlugins.build().set(ServerPlugin {
                tick_policy: TickPolicy::MaxTickRate(MAX_TICK_RATE),
                ..default()
            }),
            RepliconRenetPlugins,
            SnapshotInterpolationPlugin {
                max_tick_rate: MAX_TICK_RATE,
            },
            SimpleBoxPlugin,
        ))
        .run();
}

struct SimpleBoxPlugin;

impl Plugin for SimpleBoxPlugin {
    fn build(&self, app: &mut App) {
        app.replicate_interpolated::<PlayerPosition>()
            .replicate::<PlayerColor>()
            .add_client_predicted_event::<MoveDirection>(Channel::Ordered)
            .predict_event_for_component::<MoveDirection, MovementSystemContext, PlayerPosition>()
            .add_systems(
                Startup,
                (Self::cli_system.map(Result::unwrap), Self::init_system),
            )
            .add_observer(Self::spawn_player)
            .add_observer(Self::despawn_player)
            .add_systems(
                Update,
                (Self::draw_boxes_system, Self::input_system),
            );
    }
}

impl SimpleBoxPlugin {
    fn cli_system(
        mut commands: Commands,
        cli: Res<Cli>,
        channels: Res<RepliconChannels>,
    ) -> Result<(), Box<dyn Error>> {
        match *cli {
            Cli::SinglePlayer => {
                commands.spawn(PlayerBundle::new(
                    SERVER,
                    Vec2::ZERO,
                    bevy::color::palettes::css::GREEN.into(),
                ));
            }
            Cli::Server { port } => {
                let server_channels_config = channels.server_configs();
                let client_channels_config = channels.client_configs();

                let server = RenetServer::new(ConnectionConfig {
                    server_channels_config,
                    client_channels_config,
                    ..Default::default()
                });

                let current_time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?;
                let public_addr = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), port);
                let socket = UdpSocket::bind(public_addr)?;
                let server_config = ServerConfig {
                    current_time,
                    max_clients: 10,
                    protocol_id: PROTOCOL_ID,
                    authentication: ServerAuthentication::Unsecure,
                    public_addresses: vec![public_addr],
                };
                let transport = NetcodeServerTransport::new(server_config, socket)?;

                commands.insert_resource(server);
                commands.insert_resource(transport);

                commands.spawn((
                    Text::new("Server"),
                    TextFont {
                        font_size: 30.0,
                        ..default()
                    },
                    TextColor::WHITE,
                ));
                commands.spawn(PlayerBundle::new(
                    SERVER,
                    Vec2::ZERO,
                    bevy::color::palettes::css::GREEN.into(),
                ));
            }
            Cli::Client { port, ip } => {
                let server_channels_config = channels.server_configs();
                let client_channels_config = channels.client_configs();

                let client = RenetClient::new(ConnectionConfig {
                    server_channels_config,
                    client_channels_config,
                    ..Default::default()
                });

                let current_time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?;
                let client_id = current_time.as_millis() as u64;
                let server_addr = SocketAddr::new(ip, port);
                let socket = UdpSocket::bind((ip, 0))?;
                let authentication = ClientAuthentication::Unsecure {
                    client_id,
                    protocol_id: PROTOCOL_ID,
                    server_addr,
                    user_data: None,
                };
                let transport = NetcodeClientTransport::new(current_time, authentication, socket)?;

                commands.insert_resource(client);
                commands.insert_resource(transport);
                commands.spawn((
                    Text::new(format!("Client: {client_id:?}")),
                    TextFont {
                        font_size: 30.0,
                        ..default()
                    },
                    TextColor::WHITE,
                ));
            }
        }

        Ok(())
    }

    fn init_system(mut commands: Commands) {
        commands.spawn(Camera2d);
    }

    /// Logs server events and spawns a new player whenever a client connects.
    fn spawn_player(trigger: Trigger<OnAdd, ConnectedClient>, mut commands: Commands) {
        commands.spawn(
            PlayerBundle::new(trigger.target(), Vec2::ZERO, Color::WHITE)
        );
    }

    fn despawn_player(trigger: Trigger<OnRemove, ConnectedClient>, mut commands: Commands) {
        if let Ok(mut player) = commands.get_entity(trigger.target()) {
            player.despawn();
        } else {
            info!("No player to de-spawn")
        }
    }

    fn draw_boxes_system(mut gizmos: Gizmos, players: Query<(&PlayerPosition, &PlayerColor)>) {
        for (position, color) in &players {
            gizmos.rect_2d(
                Isometry2d::from_xy(position.x, position.y),
                Vec2::ONE * 50.0,
                color.0,
            );
        }
    }

    /// Reads player inputs and sends [`MoveCommandEvents`]
    fn input_system(input: Res<ButtonInput<KeyCode>>, mut commands: Commands) {
        let mut direction = Vec2::ZERO;
        if input.pressed(KeyCode::ArrowRight) {
            direction.x += 1.0;
        }
        if input.pressed(KeyCode::ArrowLeft) {
            direction.x -= 1.0;
        }
        if input.pressed(KeyCode::ArrowUp) {
            direction.y += 1.0;
        }
        if input.pressed(KeyCode::ArrowDown) {
            direction.y -= 1.0;
        }
        if direction != Vec2::ZERO {
            commands.client_trigger(MoveDirection(direction.normalize_or_zero()));
        }
    }
}

#[derive(Component, Serialize, Deserialize)]
struct MovementSystemContext {
    pub move_speed: f32,
}

impl Predict<MoveDirection, MovementSystemContext> for PlayerPosition {
    fn apply_event(
        &mut self,
        event: &MoveDirection,
        delta_time: f32,
        context: &MovementSystemContext,
    ) {
        self.0 += event.0 * delta_time * context.move_speed;
    }
}

const PORT: u16 = 5000;
const PROTOCOL_ID: u64 = 0;

#[derive(Debug, Parser, PartialEq, Resource)]
enum Cli {
    SinglePlayer,
    Server {
        #[arg(short, long, default_value_t = PORT)]
        port: u16,
    },
    Client {
        #[arg(short, long, default_value_t = Ipv4Addr::LOCALHOST.into())]
        ip: IpAddr,

        #[arg(short, long, default_value_t = PORT)]
        port: u16,
    },
}

impl Default for Cli {
    fn default() -> Self {
        Self::parse()
    }
}

#[derive(Bundle)]
struct PlayerBundle {
    owner: NetworkOwner,
    position: PlayerPosition,
    color: PlayerColor,
    replicated: Replicated,
    owner_predicted: OwnerPredicted,
    movement_system_context: MovementSystemContext,
}

impl PlayerBundle {
    fn new(id: Entity, position: Vec2, color: Color) -> Self {
        Self {
            owner: NetworkOwner(id),
            position: PlayerPosition(position),
            color: PlayerColor(color),
            replicated: Replicated,
            owner_predicted: OwnerPredicted,
            movement_system_context: MovementSystemContext { move_speed: 200.0 },
        }
    }
}

#[derive(Component, Deserialize, Serialize, Deref, DerefMut, Interpolate, Clone)]
struct PlayerPosition(Vec2);

#[derive(Component, Deserialize, Serialize)]
struct PlayerColor(Color);

/// A movement event for the controlled box.
#[derive(Debug, Default, Deserialize, Event, Serialize, Clone)]
struct MoveDirection(Vec2);
