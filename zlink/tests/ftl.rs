//! Test the `#[service]` attribute macro with the FTL (faster-than-light drive) example.
//!
//! This is the macro-based version of what was previously a low-level service implementation.

#![cfg(all(feature = "service", feature = "introspection", feature = "idl-parse"))]

use std::{pin::pin, time::Duration};

use futures_util::{TryStreamExt, pin_mut, stream::StreamExt};
use serde::{Deserialize, Serialize};
use tokio::{select, time::sleep};
use zlink::{
    introspect::{self, CustomType, ReplyError as _, Type},
    varlink_service::{self, Proxy as _},
};

#[cfg(all(feature = "smol", not(feature = "tokio")))]
use zlink::smol::{
    notified::{self, traits::State as _},
    unix::{bind, connect},
};
#[cfg(feature = "tokio")]
use zlink::tokio::{
    notified::{self, traits::State as _},
    unix::{bind, connect},
};

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn ftl() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let socket_path = dir.path().join(SOCKET_FILE);

    // The transitions between the drive conditions.
    let conditions = [
        DriveCondition {
            state: DriveState::Idle,
            tylium_level: 100,
        },
        DriveCondition {
            state: DriveState::Spooling,
            tylium_level: 90,
        },
        DriveCondition {
            state: DriveState::Spooling,
            tylium_level: 90,
        },
    ];

    // Set up the server and run it concurrently with the client.
    let listener = bind(&socket_path).unwrap();
    let service = Ftl::new(conditions[0]);
    let server = zlink::Server::new(listener, service);
    select! {
        res = server.run() => res?,
        res = run_client(&socket_path, &conditions) => res?,
    }

    Ok(())
}

async fn run_client(
    socket_path: &std::path::Path,
    conditions: &[DriveCondition],
) -> Result<(), Box<dyn std::error::Error>> {
    // Now create a client connection that monitors changes in the drive condition.
    let mut conn = connect(socket_path).await?;
    let mut drive_monitor_stream = pin!(conn.get_drive_condition().await?);

    // And a client that only calls methods.
    {
        let mut conn = connect(socket_path).await?;

        // Let's start with some introspection.
        let info = conn.get_info().await?.map_err(|e| e.to_string())?;
        assert_eq!(info.vendor, VENDOR);
        assert_eq!(info.product, PRODUCT);
        assert_eq!(info.version, VERSION);
        assert_eq!(info.url, URL);
        assert_eq!(info.interfaces, INTERFACES);

        // Test `org.varlink.service` interface impl.
        let interface = conn
            .get_interface_description("org.varlink.service")
            .await?
            .map_err(|e| e.to_string())?;
        let interface = interface.parse().unwrap();
        assert_eq!(&interface, varlink_service::DESCRIPTION);

        // Test `org.example.ftl` interface impl.
        let interface = conn
            .get_interface_description("org.example.ftl")
            .await?
            .map_err(|e| e.to_string())?;
        let interface = interface.parse().unwrap();
        assert_eq!(interface.name(), "org.example.ftl");
        // Verify methods are present.
        let method_names: Vec<_> = interface.methods().map(|m| m.name()).collect();
        assert!(method_names.contains(&"GetDriveCondition"));
        assert!(method_names.contains(&"SetDriveCondition"));
        assert!(method_names.contains(&"Jump"));
        assert!(method_names.contains(&"Locate"));
        assert!(method_names.contains(&"GetCoordinates"));
        assert!(method_names.contains(&"ResetCoordinates"));

        // Unimplemented interface query should return an error.
        let error = conn
            .get_interface_description("org.varlink.unimplemented")
            .await
            .unwrap_err();
        let zlink::Error::VarlinkService(owned_error) = error else {
            panic!("Expected VarlinkService error");
        };
        assert!(matches!(
            owned_error.inner(),
            varlink_service::Error::InterfaceNotFound { .. }
        ));

        // Locate a target.
        let target = "Alpha Centauri";
        let location = conn.locate(target).await.unwrap()?;
        assert_eq!(location.name, target);

        // Set the drive condition and then set it again to test chaining.
        // Use owned variants for chain methods (chain requires owned types).
        let replies = conn
            .chain_set_drive_condition(conditions[1])?
            .set_drive_condition(conditions[2])?
            .send::<OwnedFtlReply, FtlError>()
            .await?;

        // Now we should be able to get all the replies.
        {
            pin_mut!(replies);

            // First reply: confirmation of first set_drive_condition.
            let (reply, _fds) = replies.next().await.unwrap()?;
            let reply = reply.unwrap();
            let Some(OwnedFtlReply::DriveCondition(drive_condition)) = reply.into_parameters()
            else {
                panic!("Unexpected reply");
            };
            assert_eq!(drive_condition, conditions[1]);

            // Second reply: confirmation of second set_drive_condition.
            let (reply, _fds) = replies.next().await.unwrap()?;
            let reply = reply.unwrap();
            let Some(OwnedFtlReply::DriveCondition(drive_condition)) = reply.into_parameters()
            else {
                panic!("Unexpected reply");
            };
            assert_eq!(drive_condition, conditions[2]);

            // Should be no more replies.
            assert!(replies.next().await.is_none());
        }

        {
            let duration = 10;
            let impossible_speed = conditions[1].tylium_level / duration + 1;
            // Use owned variants for chain methods.
            let replies = conn
                .chain_jump(DriveConfiguration {
                    speed: impossible_speed,
                    trajectory: 1,
                    duration,
                })?
                // Now let's try to jump with a valid speed.
                .jump(DriveConfiguration {
                    speed: impossible_speed - 1,
                    trajectory: 1,
                    duration,
                })?
                .send::<OwnedFtlReply, FtlError>()
                .await?;
            pin_mut!(replies);
            let (result, _fds) = replies.try_next().await?.unwrap();
            let e = result.unwrap_err();
            // The first call should fail because we didn't have enough energy.
            assert_eq!(e, FtlError::NotEnoughEnergy);

            // The second call should succeed.
            let (reply, _fds) = replies.try_next().await?.unwrap();
            let reply = reply?;
            assert_eq!(
                reply.parameters(),
                Some(&OwnedFtlReply::Coordinates(Coordinate {
                    longitude: 1.0,
                    latitude: 0.0,
                    distance: 10,
                }))
            );
        }

        // Test oneway chain methods with a sandwich pattern to verify interleaving works.
        // After the jump test above, coordinates should be (1.0, 0.0, 10).
        // Pattern: get_coordinates -> reset_coordinates -> reset_coordinates -> get_coordinates
        // This tests:
        // 1. chain_get_coordinates() starts a chain with a regular call
        // 2. reset_coordinates() chain extension is generated for oneway methods
        // 3. Two consecutive oneway calls work correctly
        // 4. Oneway calls are actually handled (coordinates change from non-zero to zero)
        // 5. Only 2 replies are received (one per regular call, none for oneway)
        {
            let replies = conn
                .chain_get_coordinates()?
                .reset_coordinates()?
                .reset_coordinates()?
                .get_coordinates()?
                .send::<OwnedFtlReply, FtlError>()
                .await?;
            pin_mut!(replies);

            // First reply: coordinates before reset (non-zero from jump).
            let (reply, _fds) = replies.next().await.unwrap()?;
            let reply = reply.unwrap();
            let Some(OwnedFtlReply::Coordinates(coords)) = reply.into_parameters() else {
                panic!("Expected Coordinates reply");
            };
            assert_eq!(
                coords,
                Coordinate {
                    longitude: 1.0,
                    latitude: 0.0,
                    distance: 10,
                }
            );

            // Second reply: coordinates after reset (should be zero).
            let (reply, _fds) = replies.next().await.unwrap()?;
            let reply = reply.unwrap();
            let Some(OwnedFtlReply::Coordinates(coords)) = reply.into_parameters() else {
                panic!("Expected Coordinates reply");
            };
            assert_eq!(
                coords,
                Coordinate {
                    longitude: 0.0,
                    latitude: 0.0,
                    distance: 0,
                }
            );

            // No more replies - oneway calls don't generate replies.
            assert!(replies.next().await.is_none());
        }
    }

    // `drive_monitor_conn` should have received the drive condition changes.
    let drive_cond = drive_monitor_stream.try_next().await?.unwrap()?;
    let OwnedFtlReply::DriveCondition(condition) = drive_cond else {
        panic!("Expected DriveCondition reply");
    };
    assert_eq!(condition, conditions[1]);

    Ok(())
}

// Owned versions for chain API (requires DeserializeOwned).
#[derive(Debug, Clone, Deserialize, PartialEq)]
struct OwnedLocation {
    name: String,
    coordinates: Coordinate,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(untagged)]
enum OwnedFtlReply {
    DriveCondition(DriveCondition),
    Coordinates(Coordinate),
    Location(OwnedLocation),
}

#[zlink::proxy("org.example.ftl")]
trait FtlProxy {
    // Streaming method for getting/monitoring drive condition.
    // When called with more=true, returns a continuous stream of changes.
    #[zlink(more)]
    async fn get_drive_condition(
        &mut self,
    ) -> zlink::Result<
        impl futures_util::Stream<Item = zlink::Result<Result<OwnedFtlReply, FtlError>>>,
    >;

    // Regular methods can use borrowed types.
    async fn locate(&mut self, target: &str) -> zlink::Result<Result<Location, FtlError>>;

    // Owned return type variants for chain API (chain methods are generated).
    async fn set_drive_condition(
        &mut self,
        condition: DriveCondition,
    ) -> zlink::Result<Result<OwnedFtlReply, FtlError>>;

    async fn jump(
        &mut self,
        config: DriveConfiguration,
    ) -> zlink::Result<Result<OwnedFtlReply, FtlError>>;

    async fn get_coordinates(&mut self) -> zlink::Result<Result<OwnedFtlReply, FtlError>>;

    // Oneway method - no reply expected. Chain methods are generated for these.
    #[zlink(oneway)]
    async fn reset_coordinates(&mut self) -> zlink::Result<()>;
}

// ============================================================================
// The FTL service implementation using the service macro.
// ============================================================================

/// The FTL drive service.
struct Ftl {
    drive_condition: notified::State<DriveCondition, DriveCondition>,
    coordinates: Coordinate,
}

impl Ftl {
    fn new(init_conditions: DriveCondition) -> Self {
        Self {
            drive_condition: notified::State::new(init_conditions),
            coordinates: Coordinate {
                longitude: 0.0,
                latitude: 0.0,
                distance: 0,
            },
        }
    }
}

#[zlink::service(
    interface = "org.example.ftl",
    vendor = "The FL project",
    product = "FTL-capable Spaceship \u{1F680}",
    version = "1",
    url = "https://want.ftl.now/",
    types = [DriveCondition, DriveConfiguration, Coordinate, Location]
)]
impl Ftl {
    /// Get the drive condition (streaming method).
    /// Uses concrete type to avoid boxing in the generated code.
    /// When `more` is false, returns a single reply with the current condition.
    /// When `more` is true, returns a continuous stream of condition changes.
    #[zlink(more)]
    async fn get_drive_condition(&self, more: bool) -> notified::Stream<DriveCondition> {
        if more {
            self.drive_condition.stream()
        } else {
            self.drive_condition.stream_once()
        }
    }

    /// Set the drive condition.
    async fn set_drive_condition(&mut self, condition: DriveCondition) -> DriveCondition {
        self.drive_condition.set(condition).await;
        self.drive_condition.get()
    }

    /// Get the current coordinates.
    async fn get_coordinates(&self) -> Coordinate {
        self.coordinates
    }

    /// Jump to a new location based on the given configuration.
    async fn jump(&mut self, config: DriveConfiguration) -> Result<Coordinate, FtlError> {
        let condition = self.drive_condition.get();
        let tylium_required = config.speed * config.duration;
        if tylium_required > condition.tylium_level {
            return Err(FtlError::NotEnoughEnergy);
        }
        let current_coords = self.coordinates;

        sleep(Duration::from_millis(1)).await; // Simulate spooling time.

        let coords = Coordinate {
            longitude: current_coords.longitude + config.trajectory as f32,
            latitude: current_coords.latitude,
            distance: current_coords.distance + config.duration,
        };
        // Update drive condition and notify listeners.
        let new_condition = DriveCondition {
            state: DriveState::Idle,
            tylium_level: condition.tylium_level - tylium_required,
        };
        self.drive_condition.set(new_condition).await;
        self.coordinates = coords;

        Ok(coords)
    }

    /// Locate a target by name and return its coordinates.
    async fn locate(&self, target: String) -> Location {
        // Generate pseudo-random coordinates based on the target string.
        let coordinates = Coordinate {
            longitude: target.len() as f32 * 1.1,
            latitude: target.len() as f32 * 2.2,
            distance: target.len() as i64 * 10,
        };
        Location {
            name: target,
            coordinates,
        }
    }

    /// Reset coordinates to origin (oneway method).
    async fn reset_coordinates(&mut self) {
        self.coordinates = Coordinate {
            longitude: 0.0,
            latitude: 0.0,
            distance: 0,
        };
    }
}

// ============================================================================
// Data types
// ============================================================================

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, CustomType)]
struct DriveCondition {
    state: DriveState,
    tylium_level: i64,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Type)]
#[serde(rename_all = "snake_case")]
pub enum DriveState {
    Idle,
    Spooling,
    Busy,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, CustomType)]
struct DriveConfiguration {
    speed: i64,
    trajectory: i64,
    duration: i64,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, CustomType)]
struct Coordinate {
    longitude: f32,
    latitude: f32,
    distance: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, CustomType)]
struct Location {
    name: String,
    coordinates: Coordinate,
}

#[derive(Debug, Clone, PartialEq, zlink::ReplyError, introspect::ReplyError)]
#[zlink(interface = "org.example.ftl")]
enum FtlError {
    NotEnoughEnergy,
    ParameterOutOfRange,
    InvalidCoordinates {
        latitude: f32,
        longitude: f32,
        reason: String,
    },
    SystemOverheat {
        temperature: i32,
    },
}

impl core::fmt::Display for FtlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FtlError::NotEnoughEnergy => write!(f, "Not enough energy"),
            FtlError::ParameterOutOfRange => write!(f, "Parameter out of range"),
            FtlError::InvalidCoordinates {
                latitude,
                longitude,
                reason,
            } => {
                write!(
                    f,
                    "Invalid coordinates ({}, {}): {}",
                    latitude, longitude, reason
                )
            }
            FtlError::SystemOverheat { temperature } => {
                write!(f, "System overheating at {} degrees", temperature)
            }
        }
    }
}

impl std::error::Error for FtlError {}

#[test_log::test(tokio::test)]
async fn reply_error_derive_works() {
    // Test that the ReplyError derive generates the expected variants.
    assert_eq!(FtlError::VARIANTS.len(), 4);

    // Unit variants.
    assert_eq!(FtlError::VARIANTS[0].name(), "NotEnoughEnergy");
    assert!(FtlError::VARIANTS[0].has_no_fields());

    assert_eq!(FtlError::VARIANTS[1].name(), "ParameterOutOfRange");
    assert!(FtlError::VARIANTS[1].has_no_fields());

    // Variant with named fields.
    assert_eq!(FtlError::VARIANTS[2].name(), "InvalidCoordinates");
    assert!(!FtlError::VARIANTS[2].has_no_fields());
    let fields: Vec<_> = FtlError::VARIANTS[2].fields().collect();
    assert_eq!(fields.len(), 3);
    assert_eq!(fields[0].name(), "latitude");
    assert_eq!(fields[1].name(), "longitude");
    assert_eq!(fields[2].name(), "reason");

    // Another variant with named fields.
    assert_eq!(FtlError::VARIANTS[3].name(), "SystemOverheat");
    assert!(!FtlError::VARIANTS[3].has_no_fields());
    let fields: Vec<_> = FtlError::VARIANTS[3].fields().collect();
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].name(), "temperature");
}

// ============================================================================
// Constants
// ============================================================================

const SOCKET_FILE: &str = "zlink-ftl.sock";
const VENDOR: &str = "The FL project";
const PRODUCT: &str = "FTL-capable Spaceship \u{1F680}";
const VERSION: &str = "1";
const URL: &str = "https://want.ftl.now/";
const INTERFACES: [&str; 2] = ["org.example.ftl", "org.varlink.service"];
