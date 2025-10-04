#![cfg(all(feature = "introspection", feature = "idl-parse"))]

use std::{pin::pin, time::Duration};

use futures_util::{pin_mut, stream::StreamExt, TryStreamExt};
use serde::{Deserialize, Serialize};
use serde_prefix_all::prefix_all;
use tokio::{select, time::sleep};
use zlink::{
    idl::Interface,
    introspect::{self, CustomType, ReplyError as _, Type},
    notified,
    service::MethodReply,
    unix::{bind, connect},
    varlink_service::{
        self, Info, InterfaceDescription, Method as VarlinkSrvMethod, Proxy as _,
        Reply as VarlinkSrvReply,
    },
    Call, Service,
};

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn lowlevel_ftl() -> Result<(), Box<dyn std::error::Error>> {
    // Remove the socket file if it exists (from a previous run of this test).
    if let Err(e) = tokio::fs::remove_file(SOCKET_PATH).await {
        // It's OK if the file doesn't exist.
        if e.kind() != std::io::ErrorKind::NotFound {
            return Err(e.into());
        }
    }

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

    // Setup the server and run it in a separate task.
    let listener = bind(SOCKET_PATH).unwrap();
    let service = Ftl::new(conditions[0]);
    let server = zlink::Server::new(listener, service);
    select! {
        res = server.run() => res?,
        res = run_client(&conditions) => res?,
    }

    Ok(())
}

async fn run_client(conditions: &[DriveCondition]) -> Result<(), Box<dyn std::error::Error>> {
    // Now create a client connection that monitor changes in the drive condition.
    let mut conn = connect(SOCKET_PATH).await?;
    let mut drive_monitor_stream = pin!(conn.get_drive_condition_more().await?);

    // And a client that only calls methods.
    {
        let mut conn = connect(SOCKET_PATH).await?;

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
        assert_eq!(&interface, FTL_INTERFACE_DESCRIPTION);

        // Unimplemented interface query should return an error.
        let error = conn
            .get_interface_description("org.varlink.unimplemented")
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            zlink::Error::VarlinkService(varlink_service::Error::InterfaceNotFound { .. })
        ));

        // Ask for the drive condition, then set them and then ask again.
        let replies = conn
            .chain_get_drive_condition::<FtlReply, FtlError>()?
            .set_drive_condition(conditions[1])?
            .get_drive_condition()?
            .send()
            .await?;

        // Now we should be able to get all the replies.
        {
            pin_mut!(replies);

            // First reply: initial drive condition
            let reply = replies.next().await.unwrap()?.unwrap();
            let Some(FtlReply::DriveCondition(drive_condition)) = reply.into_parameters() else {
                panic!("Unexpected reply");
            };
            assert_eq!(drive_condition, conditions[0]);

            // Second reply: confirmation of set_drive_condition
            let reply = replies.next().await.unwrap()?.unwrap();
            let Some(FtlReply::DriveCondition(drive_condition)) = reply.into_parameters() else {
                panic!("Unexpected reply");
            };
            assert_eq!(drive_condition, conditions[1]);

            // Third reply: get_drive_condition after the set
            let reply = replies.next().await.unwrap()?.unwrap();
            let Some(FtlReply::DriveCondition(drive_condition)) = reply.into_parameters() else {
                panic!("Unexpected reply");
            };
            // Should match the current server state (after the set_drive_condition call)
            assert_eq!(drive_condition, conditions[1]);

            // Should be no more replies
            assert!(replies.next().await.is_none());
        }

        let duration = 10;
        let impossible_speed = conditions[1].tylium_level / duration + 1;
        let replies = conn
            // Let's try to jump to a new coordinate but first requiring more tylium
            // than we have.
            .chain_jump::<FtlReply, FtlError>(DriveConfiguration {
                speed: impossible_speed,
                trajectory: 1,
                duration: 10,
            })?
            // Now let's try to jump with a valid speed.
            .jump(DriveConfiguration {
                speed: impossible_speed - 1,
                trajectory: 1,
                duration: 10,
            })?
            .send()
            .await?;
        pin_mut!(replies);
        let e = replies.try_next().await?.unwrap().unwrap_err();
        // The first call should fail because we didn't have enough energy.
        assert_eq!(e, FtlError::NotEnoughEnergy);

        // The second call should succeed.
        let reply = replies.try_next().await?.unwrap()?;
        assert_eq!(
            reply.parameters(),
            Some(&FtlReply::Coordinates(Coordinate {
                longitude: 1.0,
                latitude: 0.0,
                distance: 10,
            }))
        );
    }

    // `drive_monitor_conn` should have received the drive condition changes.
    let drive_cond = drive_monitor_stream.try_next().await?.unwrap()?;
    let FtlReply::DriveCondition(condition) = drive_cond else {
        panic!("Expected DriveCondition reply");
    };
    assert_eq!(condition, conditions[1]);

    Ok(())
}

#[zlink::proxy("org.example.ftl")]
trait FtlProxy {
    async fn get_drive_condition(&mut self) -> zlink::Result<Result<FtlReply, FtlError>>;

    #[zlink(more, rename = "GetDriveCondition")]
    async fn get_drive_condition_more(
        &mut self,
    ) -> zlink::Result<impl futures_util::Stream<Item = zlink::Result<Result<FtlReply, FtlError>>>>;

    async fn set_drive_condition(
        &mut self,
        condition: DriveCondition,
    ) -> zlink::Result<Result<FtlReply, FtlError>>;
    async fn jump(
        &mut self,
        config: DriveConfiguration,
    ) -> zlink::Result<Result<FtlReply, FtlError>>;
}

// The FTL service.
struct Ftl {
    drive_condition: notified::State<DriveCondition, FtlReply>,
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

impl Service for Ftl {
    type MethodCall<'de> = Method<'de>;
    type ReplyParams<'ser> = Reply<'ser>;
    type ReplyStream = notified::Stream<Self::ReplyStreamParams>;
    type ReplyStreamParams = FtlReply;
    type ReplyError<'ser> = ReplyError;

    async fn handle<'ser>(
        &'ser mut self,
        call: Call<Self::MethodCall<'_>>,
    ) -> MethodReply<Self::ReplyParams<'ser>, Self::ReplyStream, Self::ReplyError<'ser>> {
        match call.method() {
            Method::Ftl(FtlMethod::GetDriveCondition) if call.more() => {
                MethodReply::Multi(self.drive_condition.stream())
            }
            Method::Ftl(FtlMethod::GetDriveCondition) => {
                MethodReply::Single(Some(Reply::Ftl(self.drive_condition.get().into())))
            }
            Method::Ftl(FtlMethod::SetDriveCondition { condition }) => {
                if call.more() {
                    return MethodReply::Error(ReplyError::Ftl(FtlError::ParameterOutOfRange));
                }
                self.drive_condition.set(*condition);
                MethodReply::Single(Some(Reply::Ftl(self.drive_condition.get().into())))
            }
            Method::Ftl(FtlMethod::GetCoordinates) => {
                MethodReply::Single(Some(Reply::Ftl(FtlReply::Coordinates(self.coordinates))))
            }
            Method::Ftl(FtlMethod::Jump { config }) => {
                if call.more() {
                    return MethodReply::Error(ReplyError::Ftl(FtlError::ParameterOutOfRange));
                }
                let tylium_required = config.speed * config.duration;
                let mut condition = self.drive_condition.get();
                if tylium_required > condition.tylium_level {
                    return MethodReply::Error(ReplyError::Ftl(FtlError::NotEnoughEnergy));
                }
                let current_coords = self.coordinates;
                let config = *config;

                sleep(Duration::from_millis(1)).await; // Simulate spooling time.

                let coords = Coordinate {
                    longitude: current_coords.longitude + config.trajectory as f32,
                    latitude: current_coords.latitude,
                    distance: current_coords.distance + config.duration,
                };
                condition.state = DriveState::Idle;
                condition.tylium_level = condition.tylium_level - tylium_required;
                self.drive_condition.set(condition);
                self.coordinates = coords;

                MethodReply::Single(Some(Reply::Ftl(FtlReply::Coordinates(coords))))
            }
            Method::VarlinkSrv(VarlinkSrvMethod::GetInfo) => {
                let interfaces = Vec::from_iter(INTERFACES.iter().cloned());
                let info = Info::new(VENDOR, PRODUCT, VERSION, URL, interfaces);

                MethodReply::Single(Some(Reply::VarlinkSrv(VarlinkSrvReply::Info(info))))
            }
            Method::VarlinkSrv(VarlinkSrvMethod::GetInterfaceDescription { interface }) => {
                let description = match *interface {
                    "org.varlink.service" => {
                        InterfaceDescription::from(varlink_service::DESCRIPTION)
                    }
                    "org.example.ftl" => InterfaceDescription::from(FTL_INTERFACE_DESCRIPTION),
                    _ => {
                        return MethodReply::Error(ReplyError::VarlinkSrv(
                            varlink_service::Error::InterfaceNotFound {
                                interface: "unknown.interface".try_into().unwrap(),
                            },
                        ))
                    }
                };

                MethodReply::Single(Some(Reply::VarlinkSrv(
                    VarlinkSrvReply::InterfaceDescription(description),
                )))
            }
        }
    }
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, CustomType)]
struct DriveCondition {
    state: DriveState,
    tylium_level: i64,
}

impl From<DriveCondition> for FtlReply {
    fn from(drive_condition: DriveCondition) -> Self {
        FtlReply::DriveCondition(drive_condition)
    }
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

impl From<Coordinate> for FtlReply {
    fn from(coordinate: Coordinate) -> Self {
        FtlReply::Coordinates(coordinate)
    }
}

//
// Aggregate types for both interfaces our service implements.
//

#[derive(Debug, Deserialize)]
#[serde(untagged)]
#[allow(unused)]
enum Method<'a> {
    Ftl(FtlMethod),
    #[serde(borrow)]
    VarlinkSrv(VarlinkSrvMethod<'a>),
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
#[allow(unused)]
enum Reply<'a> {
    Ftl(FtlReply),
    VarlinkSrv(VarlinkSrvReply<'a>),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
#[allow(unused)]
enum ReplyError {
    Ftl(FtlError),
    VarlinkSrv(varlink_service::Error),
}

//
// Types for `org.example.ftl` interface.
//

#[prefix_all("org.example.ftl.")]
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "method", content = "parameters")]
enum FtlMethod {
    GetDriveCondition,
    SetDriveCondition { condition: DriveCondition },
    GetCoordinates,
    Jump { config: DriveConfiguration },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
enum FtlReply {
    DriveCondition(DriveCondition),
    Coordinates(Coordinate),
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

    // Unit variants
    assert_eq!(FtlError::VARIANTS[0].name(), "NotEnoughEnergy");
    assert!(FtlError::VARIANTS[0].has_no_fields());

    assert_eq!(FtlError::VARIANTS[1].name(), "ParameterOutOfRange");
    assert!(FtlError::VARIANTS[1].has_no_fields());

    // Variant with named fields
    assert_eq!(FtlError::VARIANTS[2].name(), "InvalidCoordinates");
    assert!(!FtlError::VARIANTS[2].has_no_fields());
    let fields: Vec<_> = FtlError::VARIANTS[2].fields().collect();
    assert_eq!(fields.len(), 3);
    assert_eq!(fields[0].name(), "latitude");
    assert_eq!(fields[1].name(), "longitude");
    assert_eq!(fields[2].name(), "reason");

    // Another variant with named fields
    assert_eq!(FtlError::VARIANTS[3].name(), "SystemOverheat");
    assert!(!FtlError::VARIANTS[3].has_no_fields());
    let fields: Vec<_> = FtlError::VARIANTS[3].fields().collect();
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].name(), "temperature");
}

const SOCKET_PATH: &'static str = "/tmp/zlink-lowlevel-ftl.sock";

const VENDOR: &str = "The FL project";
const PRODUCT: &str = "FTL-capable Spaceship 🚀";
const VERSION: &str = "1";
const URL: &str = "https://want.ftl.now/";
const INTERFACES: [&'static str; 2] = ["org.example.ftl", "org.varlink.service"];

/// Interface definition for the FTL service.
const FTL_INTERFACE_DESCRIPTION: &Interface<'static> = &{
    use zlink::idl::{Comment, Method, Parameter};

    const MONITOR_METHOD: &Method<'static> = &{
        const OUT_PARAMS: &[&Parameter<'static>] =
            &[&Parameter::new("condition", DriveCondition::TYPE, &[])];
        Method::new(
            "Monitor",
            &[],
            OUT_PARAMS,
            &[&Comment::new("Monitor the drive condition")],
        )
    };
    const CALCULATE_CONFIGURATION_METHOD: &Method<'static> = &{
        const IN_PARAMS: &[&Parameter<'static>] = &[
            &Parameter::new("current", Coordinate::TYPE, &[]),
            &Parameter::new("target", Coordinate::TYPE, &[]),
        ];
        const OUT_PARAMS: &[&Parameter<'static>] = &[&Parameter::new(
            "configuration",
            DriveConfiguration::TYPE,
            &[],
        )];
        Method::new(
            "CalculateConfiguration",
            IN_PARAMS,
            OUT_PARAMS,
            &[&Comment::new(
                "Calculate the drive configuration for a given set of coordinates",
            )],
        )
    };
    const JUMP_METHOD: &Method<'static> = &{
        const IN_PARAMS: &[&Parameter<'static>] = &[&Parameter::new(
            "configuration",
            DriveConfiguration::TYPE,
            &[],
        )];
        Method::new(
            "Jump",
            IN_PARAMS,
            &[],
            &[&Comment::new("Jump to the calculated point in space")],
        )
    };

    Interface::new(
        "org.example.ftl",
        &[MONITOR_METHOD, CALCULATE_CONFIGURATION_METHOD, JUMP_METHOD],
        &[
            DriveCondition::CUSTOM_TYPE,
            DriveConfiguration::CUSTOM_TYPE,
            Coordinate::CUSTOM_TYPE,
        ],
        FtlError::VARIANTS,
        &[
            &Comment::new("Interface to jump a spacecraft to another point in space."),
            &Comment::new("The FTL Drive is the propulsion system to achieve"),
            &Comment::new("faster-than-light travel through space. A ship making a"),
            &Comment::new("properly calculated jump can arrive safely in planetary"),
            &Comment::new("orbit, or alongside other ships or spaceborne objects."),
        ],
    )
};
