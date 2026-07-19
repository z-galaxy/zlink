// Resolve a given hostname to an IP address using `systemd-resolved`'s Varlink service.
// We use the proxy macro to generate a type-safe client API.
use std::{env::args, fmt::Display, net::IpAddr};

use futures_util::{StreamExt, pin_mut};
use serde_repr::{Deserialize_repr, Serialize_repr};
use zlink::{ReplyError, proxy};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Setup tracing subscriber
    tracing_subscriber::fmt::init();

    let mut connection =
        zlink::tokio::unix::connect("/run/systemd/resolve/io.systemd.Resolve").await?;

    let args: Vec<_> = args().skip(1).collect();

    // Use pipelining to send all hostname resolution requests at once.
    if args.is_empty() {
        eprintln!("Usage: resolved <hostname> [<hostname> ...]");
        return Ok(());
    }

    // Build the chain of pipelined requests.
    let mut chain = connection.chain_resolve_hostname(&args[0])?;
    for name in &args[1..] {
        chain = chain.resolve_hostname(name)?;
    }

    // Send all requests at once and get back a stream of replies.
    let replies = chain.send::<ReplyParams, ReplyError>().await?;
    pin_mut!(replies);

    // Collect results and print them.
    let mut i = 0;
    while let Some(reply) = replies.next().await {
        let name = &args[i];
        i += 1;

        let (result, _fds) = reply?;
        match result {
            Ok(reply) => {
                println!("Results for '{name}':");
                for address in reply.into_parameters().unwrap().addresses {
                    println!("\t{address}");
                }
            }
            Err(e) => eprintln!("Error resolving '{name}': {e}"),
        }
    }

    Ok(())
}

#[proxy("io.systemd.Resolve")]
trait ResolvedProxy {
    #[allow(unused)]
    async fn resolve_hostname(
        &mut self,
        name: &str,
    ) -> zlink::Result<Result<ReplyParams, ReplyError>>;
}

// Owned types (required by chain API which needs DeserializeOwned).
#[derive(Debug, serde::Deserialize)]
struct ReplyParams {
    addresses: Vec<ResolvedAddress>,
    #[serde(rename = "name")]
    _name: String,
}

#[derive(Debug, serde::Deserialize)]
struct ResolvedAddress {
    family: ProtocolFamily,
    address: Vec<u8>,
}

impl Display for ResolvedAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ip = match self.family {
            ProtocolFamily::Inet => {
                let ip = <[u8; 4]>::try_from(self.address.as_slice())
                    .map(IpAddr::from)
                    .unwrap();
                format!("IPv4: {ip}")
            }
            ProtocolFamily::Inet6 => {
                let ip = <[u8; 16]>::try_from(self.address.as_slice())
                    .map(IpAddr::from)
                    .unwrap();
                format!("IPv6: {ip}")
            }
            ProtocolFamily::Unspec => {
                format!("Unspecified protocol family: {:?}", self.address)
            }
        };
        write!(f, "{ip}")
    }
}

#[derive(Serialize_repr, Deserialize_repr, PartialEq, Debug)]
#[repr(u8)]
enum ProtocolFamily {
    Unspec = 0, // Unspecified.
    Inet = 2,   // IP protocol family.
    Inet6 = 10, // IP version 6.
}

#[derive(Debug, ReplyError)]
#[zlink(interface = "io.systemd.Resolve")]
enum ReplyError {
    NoNameServers,
    NoSuchResourceRecord,
    QueryTimedOut,
    MaxAttemptsReached,
    InvalidReply,
    QueryAborted,
    DNSSECValidationFailed {
        #[zlink(rename = "result")]
        _result: String,
        #[zlink(rename = "extendedDNSErrorCode")]
        _extended_dns_error_code: Option<i32>,
        #[zlink(rename = "extendedDNSErrorMessage")]
        _extended_dns_error_message: Option<String>,
    },
    NoTrustAnchor,
    ResourceRecordTypeUnsupported,
    NetworkDown,
    NoSource,
    StubLoop,
    DNSError {
        #[zlink(rename = "rcode")]
        _rcode: i32,
        #[zlink(rename = "extendedDNSErrorCode")]
        _extended_dns_error_code: Option<i32>,
        #[zlink(rename = "extendedDNSErrorMessage")]
        _extended_dns_error_message: Option<String>,
    },
    CNAMELoop,
    BadAddressSize,
    ResourceRecordTypeInvalidForQuery,
    ZoneTransfersNotPermitted,
    ResourceRecordTypeObsolete,
}

impl Display for ReplyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for ReplyError {}
