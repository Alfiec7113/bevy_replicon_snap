//! This is the "Simple Box" example from the bevy_replicon repo with minimal modifications

use std::{
    error::Error,
    net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
    time::SystemTime,
};

use bevy::{prelude::*, winit::UpdateMode::Continuous, winit::WinitSettings};
use clap::Parser;
use serde::{Deserialize, Serialize};

use bevy_replicon::prelude::*;
use bevy_replicon_renet::{
    netcode::{
        ClientAuthentication, NetcodeClientTransport, NetcodeServerTransport, ServerAuthentication,
        ServerConfig,
    },
    renet::{ConnectionConfig, RenetClient, RenetServer},
    RenetChannelsExt, RepliconRenetPlugins,
};
use bevy_replicon_renet::renet::{ClientId, ServerEvent};

// Setting a overly low server tickrate to make the difference between the different methods clearly visible
// Usually you would want a server for a realtime game to run with at least 30 ticks per second
const MAX_TICK_RATE: u16 = 5;

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
            SimpleBoxPlugin,
        ))
        .run();
}

struct SimpleBoxPlugin;

impl Plugin for SimpleBoxPlugin {
    fn build(&self, app: &mut App) {
        app.replicate::<PlayerPosition>()
            .replicate::<PlayerColor>()
            .add_client_event::<MoveDirection>(Channel::Ordered)
            .add_systems(
                Startup,
                (Self::cli_system.map(Result::unwrap), Self::init_system),
            )
            .add_systems(
                Update,
                (
                    Self::movement_system.run_if(server_or_singleplayer), // Runs only on the server or a single player.
                    Self::server_event_system.run_if(resource_exists::<RenetServer>), // Runs only on the server.
                    (Self::draw_boxes_system, Self::input_system),
                ),
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
                    0,
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
                    0,
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
    fn server_event_system(mut commands: Commands, mut server_event: EventReader<ServerEvent>) {
        for event in server_event.read() {
            match event {
                ServerEvent::ClientConnected { client_id } => {
                    info!("player: {client_id:?} Connected");
                    // Generate pseudo random color from client id.
                    let r = ((client_id % 23) as f32) / 23.0;
                    let g = ((client_id % 27) as f32) / 27.0;
                    let b = ((client_id % 39) as f32) / 39.0;
                    commands.spawn(PlayerBundle::new(
                        *client_id,
                        Vec2::ZERO,
                        Color::srgb(r, g, b),
                    ));
                }
                ServerEvent::ClientDisconnected { client_id, reason } => {
                    info!("client {client_id:?} disconnected: {reason}");
                }
            }
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
    fn input_system(mut move_events: EventWriter<MoveDirection>, input: Res<ButtonInput<KeyCode>>) {
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
            move_events.write(MoveDirection(direction.normalize_or_zero()));
        }
    }

    /// Mutates [`PlayerPosition`] based on [`MoveCommandEvents`].
    ///
    /// Fast-paced games usually you don't want to wait until server send a position back because of the latency.
    /// But this example just demonstrates simple replication concept.
    fn movement_system(
        mut trigger: Trigger<FromClient<MoveDirection>>,
        time: Res<Time>,
        mut players: Query<(&Player, &mut PlayerPosition)>,
    ) {
        const MOVE_SPEED: f32 = 300.0;
        info!("received event {event:?} from client {client_id:?}");
        let (_, position) = players.iter_mut().find(|&(owner, _)|
            owner.0 == trigger.client_entity
        )
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
    player: Player,
    position: PlayerPosition,
    color: PlayerColor,
    replicated: Replicated,
}

impl PlayerBundle {
    fn new(client_id: ClientId, position: Vec2, color: Color) -> Self {
        Self {
            player: Player(client_id),
            position: PlayerPosition(position),
            color: PlayerColor(color),
            replicated: Replicated,
        }
    }
}

/// Contains the client ID of the player.
#[derive(Component, Serialize, Deserialize)]
struct Player(ClientId);

#[derive(Component, Deserialize, Serialize, Deref, DerefMut)]
struct PlayerPosition(Vec2);

#[derive(Component, Deserialize, Serialize)]
struct PlayerColor(Color);

/// A movement event for the controlled box.
#[derive(Debug, Default, Deserialize, Event, Serialize)]
struct MoveDirection(Vec2);
