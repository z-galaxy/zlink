//! Mock systemd-machined service for testing when real systemd services aren't available.

#![cfg(all(feature = "introspection", feature = "idl-parse"))]

use std::borrow::Cow;

use serde::{Deserialize, Serialize};
use serde_prefix_all::prefix_all;
use std::vec::Vec;
use zlink::{
    idl::{self, Comment, Interface, Parameter},
    introspect::{self, CustomType, ReplyError as _, Type},
    service::MethodReply,
    varlink_service::{self, Error, Info, InterfaceDescription},
    Call, ReplyError, Service,
};

/// Mock systemd-machined service that serves hardcoded responses.
pub struct MockMachinedService;

impl MockMachinedService {
    /// Create a new mock machined service.
    pub fn new() -> Self {
        Self
    }
}

/// Combined method enum for both varlink service and Machine methods.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum Method<'a> {
    #[serde(borrow)]
    VarlinkService(varlink_service::Method<'a>),
    #[serde(borrow)]
    Machine(MachineMethod<'a>),
}

/// Machine interface methods.
#[prefix_all("io.systemd.Machine.")]
#[derive(Debug, Deserialize)]
#[serde(tag = "method", content = "parameters")]
#[allow(dead_code)]
pub enum MachineMethod<'a> {
    Register {
        name: &'a str,
        id: Option<&'a str>,
        service: Option<&'a str>,
        class: &'a str,
        leader: Option<u32>,
        #[serde(rename = "leaderProcessId")]
        leader_process_id: Option<ProcessId<'a>>,
        #[serde(rename = "rootDirectory")]
        root_directory: Option<&'a str>,
        #[serde(rename = "ifIndices")]
        if_indices: Option<Vec<u64>>,
        #[serde(rename = "vSockCid")]
        v_sock_cid: Option<u64>,
        #[serde(rename = "sshAddress")]
        ssh_address: Option<&'a str>,
        #[serde(rename = "sshPrivateKeyPath")]
        ssh_private_key_path: Option<&'a str>,
        #[serde(rename = "allocateUnit")]
        allocate_unit: Option<bool>,
        #[serde(rename = "allowInteractiveAuthentication")]
        allow_interactive_authentication: Option<bool>,
    },
    Unregister {
        name: Option<&'a str>,
        pid: Option<ProcessId<'a>>,
        #[serde(rename = "allowInteractiveAuthentication")]
        allow_interactive_authentication: Option<bool>,
    },
    Terminate {
        name: Option<&'a str>,
        pid: Option<ProcessId<'a>>,
        #[serde(rename = "allowInteractiveAuthentication")]
        allow_interactive_authentication: Option<bool>,
    },
    Kill {
        name: Option<&'a str>,
        pid: Option<ProcessId<'a>>,
        #[serde(rename = "allowInteractiveAuthentication")]
        allow_interactive_authentication: Option<bool>,
        whom: Option<&'a str>,
        signal: i64,
    },
    List {
        name: Option<&'a str>,
        pid: Option<ProcessId<'a>>,
        #[serde(rename = "allowInteractiveAuthentication")]
        allow_interactive_authentication: Option<bool>,
        #[serde(rename = "acquireMetadata")]
        acquire_metadata: Option<AcquireMetadata>,
    },
    Open {
        name: Option<&'a str>,
        pid: Option<ProcessId<'a>>,
        #[serde(rename = "allowInteractiveAuthentication")]
        allow_interactive_authentication: Option<bool>,
        mode: MachineOpenMode,
        user: Option<&'a str>,
        path: Option<&'a str>,
        args: Option<Vec<&'a str>>,
        environment: Option<Vec<&'a str>>,
    },
}

/// Combined reply enum for both varlink service and Machine replies.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum Reply<'ser> {
    #[serde(borrow)]
    VarlinkService(varlink_service::Reply<'ser>),
    Machine(MachineReply<'ser>),
}

/// Machine interface replies.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum MachineReply<'a> {
    List(ListReply<'a>),
    Open(OpenReply),
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ListReply<'a> {
    pub name: &'a str,
    pub id: Option<&'a str>,
    pub service: Option<&'a str>,
    pub class: &'a str,
    pub leader: Option<ProcessId<'a>>,
    #[serde(rename = "rootDirectory")]
    pub root_directory: Option<&'a str>,
    // Needs owned variant for deserializing because of escaped content.
    pub unit: Option<Cow<'a, str>>,
    pub timestamp: Option<Timestamp>,
    #[serde(rename = "vSockCid")]
    pub v_sock_cid: Option<u64>,
    #[serde(rename = "sshAddress")]
    pub ssh_address: Option<&'a str>,
    #[serde(rename = "sshPrivateKeyPath")]
    pub ssh_private_key_path: Option<&'a str>,
    pub addresses: Option<Vec<Address>>,
    #[serde(rename = "OSRelease")]
    pub os_release: Option<Vec<&'a str>>,
    #[serde(rename = "UIDShift")]
    pub uid_shift: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Type)]
pub struct OpenReply {
    #[serde(rename = "ptyFileDescriptor")]
    pub pty_file_descriptor: i64,
    #[serde(rename = "ptyPath")]
    pub pty_path: String,
}

// Custom types for the Machine interface
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, CustomType)]
#[serde(rename_all = "lowercase")]
pub enum AcquireMetadata {
    No,
    Yes,
    Graceful,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, CustomType)]
#[serde(rename_all = "lowercase")]
pub enum MachineOpenMode {
    Tty,
    Login,
    Shell,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, CustomType)]
pub struct ProcessId<'a> {
    pub pid: i64,
    #[serde(rename = "pidfdId")]
    pub pidfd_id: Option<u64>,
    #[serde(rename = "bootId")]
    // According to the IDL, this should be a number but we actually get a string.
    // See https://github.com/systemd/systemd/issues/38276
    pub boot_id: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, CustomType)]
pub struct Timestamp {
    pub realtime: Option<u64>,
    pub monotonic: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, CustomType)]
pub struct Address {
    pub ifindex: Option<u64>,
    pub family: i64,
    pub address: Vec<u64>,
}

impl Service for MockMachinedService {
    type MethodCall<'de> = Method<'de>;
    type ReplyParams<'ser> = Reply<'ser>;
    type ReplyStream = futures_util::stream::Empty<zlink::Reply<()>>;
    type ReplyStreamParams = ();
    type ReplyError<'ser> = MockError;

    async fn handle<'ser>(
        &'ser mut self,
        call: Call<Self::MethodCall<'_>>,
    ) -> MethodReply<Self::ReplyParams<'ser>, Self::ReplyStream, Self::ReplyError<'ser>> {
        match call.method() {
            Method::VarlinkService(varlink_service::Method::GetInfo) => {
                // Return hardcoded info that matches the systemd machine service
                let mut interfaces = Vec::new();
                let interface_list = [
                    "io.systemd",
                    "io.systemd.Machine",
                    "io.systemd.MachineImage",
                    "org.varlink.service",
                ];
                for interface in interface_list {
                    interfaces.push(interface);
                }

                let info = Info::new(
                    "The systemd Project",
                    "systemd (systemd-machined)",
                    "257.5 (257.5-6.fc42)",
                    "https://systemd.io/",
                    interfaces,
                );

                MethodReply::Single(Some(Reply::VarlinkService(varlink_service::Reply::Info(
                    info,
                ))))
            }
            Method::VarlinkService(varlink_service::Method::GetInterfaceDescription {
                interface,
            }) => {
                let description = match *interface {
                    "org.varlink.service" => {
                        InterfaceDescription::from(varlink_service::DESCRIPTION)
                    }
                    "io.systemd.Machine" => InterfaceDescription::from(MACHINE_SERVICE_DESCRIPTION),
                    _ => {
                        return MethodReply::Error(MockError::VarlinkService(
                            Error::InterfaceNotFound {
                                interface: "unknown.interface".try_into().unwrap(),
                            },
                        ))
                    }
                };

                MethodReply::Single(Some(Reply::VarlinkService(
                    varlink_service::Reply::InterfaceDescription(description),
                )))
            }
            Method::Machine(MachineMethod::Register { .. }) => {
                // For the mock, just return success (no parameters)
                MethodReply::Single(None)
            }
            Method::Machine(MachineMethod::Unregister { .. }) => {
                // For the mock, just return success (no parameters)
                MethodReply::Single(None)
            }
            Method::Machine(MachineMethod::Terminate { .. }) => {
                // For the mock, just return success (no parameters)
                MethodReply::Single(None)
            }
            Method::Machine(MachineMethod::Kill { .. }) => {
                // For the mock, just return success (no parameters)
                MethodReply::Single(None)
            }
            Method::Machine(MachineMethod::List { .. }) => {
                // Return a mock machine
                let list_reply = MOCK_LIST_REPLY.clone();
                MethodReply::Single(Some(Reply::Machine(MachineReply::List(list_reply))))
            }
            Method::Machine(MachineMethod::Open { .. }) => {
                // Return a mock PTY
                let open_reply = OpenReply {
                    pty_file_descriptor: 42,
                    pty_path: "/dev/pts/42".to_string(),
                };
                MethodReply::Single(Some(Reply::Machine(MachineReply::Open(open_reply))))
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
#[allow(unused)]
pub enum MockError {
    VarlinkService(Error),
    Machined(MachinedError),
}

/// Errors that can be returned by the `io.systemd.Machine` interface.
#[derive(Debug, Clone, PartialEq, ReplyError, introspect::ReplyError)]
#[zlink(interface = "io.systemd.Machine")]
#[allow(unused)]
pub enum MachinedError {
    /// No matching machine currently running.
    NoSuchMachine,
    /// Machine already exists.
    MachineExists,
    /// Machine does not use private networking.
    NoPrivateNetworking,
    /// Machine does not contain OS release information.
    NoOSReleaseInformation,
    /// Machine uses a complex UID/GID mapping, cannot determine shift.
    NoUIDShift,
    /// Requested information is not available.
    NotAvailable,
    /// Requested operation is not supported.
    NotSupported,
    /// There is no IPC service (such as system bus or varlink) in the container.
    NoIPC,
}

impl core::fmt::Display for MachinedError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            MachinedError::NoSuchMachine => write!(f, "No such machine"),
            MachinedError::MachineExists => write!(f, "Machine already exists"),
            MachinedError::NoPrivateNetworking => {
                write!(f, "Machine does not use private networking")
            }
            MachinedError::NoOSReleaseInformation => {
                write!(f, "Machine does not contain OS release information")
            }
            MachinedError::NoUIDShift => write!(
                f,
                "Machine uses a complex UID/GID mapping, cannot determine shift"
            ),
            MachinedError::NotAvailable => write!(f, "Requested information is not available"),
            MachinedError::NotSupported => write!(f, "Requested operation is not supported"),
            MachinedError::NoIPC => write!(f, "There is no IPC service in the container"),
        }
    }
}

impl core::error::Error for MachinedError {}

const MOCK_LIST_REPLY: ListReply<'static> = ListReply {
    name: ".host",
    id: Some("1234567890abcdef1234567890abcdef"),
    service: Some("mock-service"),
    class: "host",
    leader: Some(ProcessId {
        pid: 12345,
        pidfd_id: None,
        boot_id: None,
    }),
    root_directory: Some("/var/lib/machines/test-machine"),
    unit: Some(Cow::Borrowed("machine-test\\x2dmachine.scope")), // Needs escaping
    timestamp: Some(Timestamp {
        realtime: Some(1234567890000000),
        monotonic: Some(9876543210000),
    }),
    v_sock_cid: None,
    ssh_address: None,
    ssh_private_key_path: None,
    addresses: None,
    os_release: None,
    uid_shift: None,
};

/// Interface definition for io.systemd.Machine matching the actual systemd-machined service.
const MACHINE_SERVICE_DESCRIPTION: &Interface<'static> = &{
    const PROCESS_ID_TYPE: &idl::Type<'static> = <Option<ProcessId>>::TYPE;
    const ACQUIRE_METADATA_TYPE: &idl::Type<'static> = <Option<AcquireMetadata>>::TYPE;

    // Method definitions with scoped parameters
    const REGISTER_METHOD: &idl::Method<'static> = &{
        const PARAMS: &[&Parameter<'static>] = &[
            &Parameter::new("name", <&str>::TYPE, &[]),
            &Parameter::new("id", <Option<&str>>::TYPE, &[]),
            &Parameter::new("service", <Option<&str>>::TYPE, &[]),
            &Parameter::new("class", <&str>::TYPE, &[]),
            &Parameter::new(
                "leader",
                <Option<u32>>::TYPE,
                &[&Comment::new("The leader PID as simple positive integer.")],
            ),
            &Parameter::new(
                "leaderProcessId",
                PROCESS_ID_TYPE,
                &[&Comment::new("The leader PID as ProcessId structure.")],
            ),
            &Parameter::new("rootDirectory", <Option<&str>>::TYPE, &[]),
            &Parameter::new("ifIndices", <Option<&[u64]>>::TYPE, &[]),
            &Parameter::new("vSockCid", <Option<u64>>::TYPE, &[]),
            &Parameter::new("sshAddress", <Option<&str>>::TYPE, &[]),
            &Parameter::new("sshPrivateKeyPath", <Option<&str>>::TYPE, &[]),
            &Parameter::new(
                "allocateUnit",
                <Option<bool>>::TYPE,
                &[&Comment::new(
                    "Controls whether to allocate a scope unit for the machine to register.",
                )],
            ),
            &Parameter::new(
                "allowInteractiveAuthentication",
                <Option<bool>>::TYPE,
                &[&Comment::new("Controls whether interactive authentication (via polkit) shall be allowed. If unspecified defaults to false")]
            ),
        ];
        idl::Method::new("Register", PARAMS, &[], &[])
    };

    const UNREGISTER_METHOD: &idl::Method<'static> = &{
        const PARAMS: &[&Parameter<'static>] = &[
            &Parameter::new(
                "name",
                <Option<&str>>::TYPE,
                &[&Comment::new("If non-null the name of a machine")],
            ),
            &Parameter::new(
                "pid",
                PROCESS_ID_TYPE,
                &[&Comment::new("If non-null the PID of a machine. Special value 0 means to take pid of the machine the caller is part of")]
            ),
            &Parameter::new(
                "allowInteractiveAuthentication",
                <Option<bool>>::TYPE,
                &[&Comment::new("Controls whether interactive authentication (via polkit) shall be allowed. If unspecified defaults to false")]
            ),
        ];
        idl::Method::new("Unregister", PARAMS, &[], &[])
    };

    const TERMINATE_METHOD: &idl::Method<'static> = &{
        const PARAMS: &[&Parameter<'static>] = &[
            &Parameter::new(
                "name",
                <Option<&str>>::TYPE,
                &[&Comment::new("If non-null the name of a machine")],
            ),
            &Parameter::new(
                "pid",
                PROCESS_ID_TYPE,
                &[&Comment::new("If non-null the PID of a machine. Special value 0 means to take pid of the machine the caller is part of")]
            ),
            &Parameter::new(
                "allowInteractiveAuthentication",
                <Option<bool>>::TYPE,
                &[&Comment::new("Controls whether interactive authentication (via polkit) shall be allowed. If unspecified defaults to false")]
            ),
        ];
        idl::Method::new(
            "Terminate",
            PARAMS,
            &[],
            &[&Comment::new("Terminate machine, killing its processes")],
        )
    };

    const KILL_METHOD: &idl::Method<'static> = &{
        const PARAMS: &[&Parameter<'static>] = &[
            &Parameter::new(
                "name",
                <Option<&str>>::TYPE,
                &[&Comment::new("If non-null the name of a machine")],
            ),
            &Parameter::new(
                "pid",
                PROCESS_ID_TYPE,
                &[&Comment::new("If non-null the PID of a machine. Special value 0 means to take pid of the machine the caller is part of")]
            ),
            &Parameter::new(
                "allowInteractiveAuthentication",
                <Option<bool>>::TYPE,
                &[&Comment::new("Controls whether interactive authentication (via polkit) shall be allowed. If unspecified defaults to false")]
            ),
            &Parameter::new(
                "whom",
                <Option<&str>>::TYPE,
                &[&Comment::new("Identifier that specifies what precisely to send the signal to (either 'leader' or 'all').")]
            ),
            &Parameter::new(
                "signal",
                <i64>::TYPE,
                &[&Comment::new("Numeric UNIX signal integer.")],
            ),
        ];
        idl::Method::new(
            "Kill",
            PARAMS,
            &[],
            &[&Comment::new(
                "Send a UNIX signal to the machine's processes",
            )],
        )
    };

    const LIST_METHOD: &idl::Method<'static> = &{
        const PARAMS: &[&Parameter<'static>] = &[
            &Parameter::new(
                "name",
                <Option<&str>>::TYPE,
                &[&Comment::new("If non-null the name of a machine")],
            ),
            &Parameter::new(
                "pid",
                PROCESS_ID_TYPE,
                &[&Comment::new("If non-null the PID of a machine. Special value 0 means to take pid of the machine the caller is part of")]
            ),
            &Parameter::new(
                "allowInteractiveAuthentication",
                <Option<bool>>::TYPE,
                &[&Comment::new("Controls whether interactive authentication (via polkit) shall be allowed. If unspecified defaults to false")]
            ),
            &Parameter::new(
                "acquireMetadata",
                ACQUIRE_METADATA_TYPE,
                &[&Comment::new("If 'yes' the output will include machine metadata fields such as 'Addresses', 'OSRelease', and 'UIDShift'. If 'graceful' it's equal to true but gracefully eats up errors")]
            ),
        ];

        // Use the ListReply TYPE to get the fields
        let output_params = ListReply::TYPE.as_object().unwrap().as_borrowed().unwrap();

        idl::Method::new(
            "List",
            PARAMS,
            output_params,
            &[
                &Comment::new("List running machines"),
                &Comment::new("[Supports 'more' flag]"),
            ],
        )
    };

    const OPEN_METHOD: &idl::Method<'static> = &{
        const PARAMS: &[&Parameter<'static>] = &[
            &Parameter::new(
                "name",
                <Option<&str>>::TYPE,
                &[&Comment::new("If non-null the name of a machine")],
            ),
            &Parameter::new(
                "pid",
                PROCESS_ID_TYPE,
                &[&Comment::new("If non-null the PID of a machine. Special value 0 means to take pid of the machine the caller is part of")]
            ),
            &Parameter::new(
                "allowInteractiveAuthentication",
                <Option<bool>>::TYPE,
                &[&Comment::new("Controls whether interactive authentication (via polkit) shall be allowed. If unspecified defaults to false")]
            ),
            &Parameter::new(
                "mode",
                MachineOpenMode::TYPE,
                &[&Comment::new(
                    "There are three possible values: 'tty', 'login', and 'shell'.",
                )],
            ),
            &Parameter::new(
                "user",
                <Option<&str>>::TYPE,
                &[&Comment::new(
                    "See description of mode='shell'. Valid only when mode='shell'",
                )],
            ),
            &Parameter::new(
                "path",
                <Option<&str>>::TYPE,
                &[&Comment::new(
                    "See description of mode='shell'. Valid only when mode='shell'",
                )],
            ),
            &Parameter::new(
                "args",
                <Option<&[&str]>>::TYPE,
                &[&Comment::new(
                    "See description of mode='shell'. Valid only when mode='shell'",
                )],
            ),
            &Parameter::new(
                "environment",
                <Option<&[&str]>>::TYPE,
                &[&Comment::new(
                    "See description of mode='shell'. Valid only when mode='shell'",
                )],
            ),
        ];

        // Use the OpenReply TYPE to get the fields
        let output_params = OpenReply::TYPE.as_object().unwrap().as_borrowed().unwrap();

        idl::Method::new(
            "Open",
            PARAMS,
            output_params,
            &[&Comment::new(
                "Allocates a pseudo TTY in the container in various modes",
            )],
        )
    };

    const METHODS: &[&idl::Method<'static>] = &[
        REGISTER_METHOD,
        UNREGISTER_METHOD,
        TERMINATE_METHOD,
        KILL_METHOD,
        LIST_METHOD,
        OPEN_METHOD,
    ];

    // Use the custom types from their CUSTOM_TYPE implementations
    const CUSTOM_TYPES: &[&idl::CustomType<'static>] = &[
        AcquireMetadata::CUSTOM_TYPE,
        MachineOpenMode::CUSTOM_TYPE,
        ProcessId::CUSTOM_TYPE,
        Timestamp::CUSTOM_TYPE,
        Address::CUSTOM_TYPE,
    ];

    Interface::new(
        "io.systemd.Machine",
        METHODS,
        CUSTOM_TYPES,
        MachinedError::VARIANTS,
        &[&Comment::new("systemd machine management interface")],
    )
};
