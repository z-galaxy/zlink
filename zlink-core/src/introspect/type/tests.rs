//! Tests for Type trait implementations.

use super::*;
use crate::idl;

#[test]
fn primitive_type() {
    assert_eq!(*bool::TYPE, idl::Type::Bool);
    assert_eq!(*i32::TYPE, idl::Type::Int);
    assert_eq!(*f64::TYPE, idl::Type::Float);
    assert_eq!(*<&str>::TYPE, idl::Type::String);
    assert_eq!(*String::TYPE, idl::Type::String);
}

#[test]
fn optional_type() {
    match <Option<i32>>::TYPE {
        idl::Type::Optional(optional) => assert_eq!(**optional, idl::Type::Int),
        _ => panic!("Expected optional type"),
    }
}

#[test]
fn array_type() {
    use std::vec::Vec;
    match <Vec<String>>::TYPE {
        idl::Type::Array(array) => assert_eq!(**array, idl::Type::String),
        _ => panic!("Expected array type"),
    }
}

#[test]
fn complex_type() {
    use std::vec::Vec;
    // Test Option<Vec<bool>>
    match <Option<Vec<bool>>>::TYPE {
        idl::Type::Optional(optional) => match &**optional {
            idl::Type::Array(array) => assert_eq!(**array, idl::Type::Bool),
            _ => panic!("Expected array inside optional"),
        },
        _ => panic!("Expected optional type"),
    }
}

#[test]
fn map_types() {
    use std::collections::{BTreeMap, HashMap};
    // Test HashMap<String, i32>
    match <HashMap<String, i32>>::TYPE {
        idl::Type::Map(value_type) => assert_eq!(**value_type, idl::Type::Int),
        _ => panic!("Expected map type"),
    }

    // Test BTreeMap<&str, bool>
    match <BTreeMap<&str, bool>>::TYPE {
        idl::Type::Map(value_type) => assert_eq!(**value_type, idl::Type::Bool),
        _ => panic!("Expected map type"),
    }
}

#[test]
fn set_types() {
    use std::collections::{BTreeSet, HashSet};
    // Test HashSet<String>
    match <HashSet<String>>::TYPE {
        idl::Type::Array(element_type) => assert_eq!(**element_type, idl::Type::String),
        _ => panic!("Expected array type"),
    }

    // Test BTreeSet<i32>
    match <BTreeSet<i32>>::TYPE {
        idl::Type::Array(element_type) => assert_eq!(**element_type, idl::Type::Int),
        _ => panic!("Expected array type"),
    }
}

#[test]
fn smart_pointer_types() {
    use std::{boxed::Box, rc::Rc, sync::Arc};
    // Test Box<bool>
    assert_eq!(*<Box<bool>>::TYPE, idl::Type::Bool);

    // Test Arc<String>
    assert_eq!(*<Arc<String>>::TYPE, idl::Type::String);

    // Test Rc<i32>
    assert_eq!(*<Rc<i32>>::TYPE, idl::Type::Int);
}

#[test]
fn cell_types() {
    use std::cell::{Cell, RefCell};
    // Test Cell<f64>
    assert_eq!(*<Cell<f64>>::TYPE, idl::Type::Float);

    // Test RefCell<bool>
    assert_eq!(*<RefCell<bool>>::TYPE, idl::Type::Bool);
}

#[test]
fn additional_numeric_types() {
    assert_eq!(*isize::TYPE, idl::Type::Int);
    assert_eq!(*usize::TYPE, idl::Type::Int);
}

#[test]
fn char_type() {
    assert_eq!(*char::TYPE, idl::Type::String);
}

#[test]
fn unit_type() {
    match <()>::TYPE {
        idl::Type::Object(fields) => assert_eq!(fields.iter().count(), 0),
        _ => panic!("Expected empty object type"),
    }
}

#[test]
fn core_time_types() {
    // Test core::time::Duration (available in no-std)
    assert_eq!(*<core::time::Duration>::TYPE, idl::Type::Float);
}

#[test]
fn std_time_types() {
    use std::time::{Instant, SystemTime};
    // Test std-only time types
    assert_eq!(*<Instant>::TYPE, idl::Type::Float);
    assert_eq!(*<SystemTime>::TYPE, idl::Type::Float);
}

#[test]
fn path_types() {
    use std::path::{Path, PathBuf};
    // Test path types
    assert_eq!(*<PathBuf>::TYPE, idl::Type::String);
    assert_eq!(*<Path>::TYPE, idl::Type::String);
}

#[test]
fn osstring_types() {
    use std::ffi::{OsStr, OsString};
    // Test OsString types
    assert_eq!(*<OsString>::TYPE, idl::Type::String);
    assert_eq!(*<OsStr>::TYPE, idl::Type::String);
}

#[test]
fn net_types() {
    use core::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
    // Test network address types (available in core)
    assert_eq!(*<IpAddr>::TYPE, idl::Type::String);
    assert_eq!(*<Ipv4Addr>::TYPE, idl::Type::String);
    assert_eq!(*<Ipv6Addr>::TYPE, idl::Type::String);
    assert_eq!(*<SocketAddr>::TYPE, idl::Type::String);
    assert_eq!(*<SocketAddrV4>::TYPE, idl::Type::String);
    assert_eq!(*<SocketAddrV6>::TYPE, idl::Type::String);
}

#[cfg(feature = "uuid")]
#[test]
fn uuid_type() {
    assert_eq!(*<uuid::Uuid>::TYPE, idl::Type::String);
}

#[cfg(feature = "url")]
#[test]
fn url_type() {
    assert_eq!(*<url::Url>::TYPE, idl::Type::String);
}

#[cfg(feature = "bytes")]
#[test]
fn bytes_types() {
    use bytes::{Bytes, BytesMut};
    assert_eq!(*<Bytes>::TYPE, idl::Type::String);
    assert_eq!(*<BytesMut>::TYPE, idl::Type::String);
}

#[cfg(feature = "indexmap")]
#[test]
fn indexmap_types() {
    use indexmap::{IndexMap, IndexSet};
    // Test IndexMap
    match <IndexMap<String, i32>>::TYPE {
        idl::Type::Map(value_type) => assert_eq!(**value_type, idl::Type::Int),
        _ => panic!("Expected map type"),
    }

    match <IndexMap<&str, bool>>::TYPE {
        idl::Type::Map(value_type) => assert_eq!(**value_type, idl::Type::Bool),
        _ => panic!("Expected map type"),
    }

    // Test IndexSet
    match <IndexSet<String>>::TYPE {
        idl::Type::Array(element_type) => assert_eq!(**element_type, idl::Type::String),
        _ => panic!("Expected array type"),
    }
}

#[cfg(feature = "chrono")]
#[test]
fn chrono_types() {
    use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, NaiveTime, Utc};
    assert_eq!(*<NaiveDate>::TYPE, idl::Type::String);
    assert_eq!(*<NaiveTime>::TYPE, idl::Type::String);
    assert_eq!(*<NaiveDateTime>::TYPE, idl::Type::String);
    assert_eq!(*<DateTime<Utc>>::TYPE, idl::Type::String);
    assert_eq!(*<Duration>::TYPE, idl::Type::Int);
}

#[cfg(feature = "time")]
#[test]
fn time_crate_types() {
    use time::{Date, Duration, OffsetDateTime, PrimitiveDateTime, Time};
    assert_eq!(*<Date>::TYPE, idl::Type::String);
    assert_eq!(*<Time>::TYPE, idl::Type::String);
    assert_eq!(*<PrimitiveDateTime>::TYPE, idl::Type::String);
    assert_eq!(*<OffsetDateTime>::TYPE, idl::Type::String);
    assert_eq!(*<Duration>::TYPE, idl::Type::Float);
}
