// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The server mapper (port 449, never TLS). The client sends a service
//! name, nothing else; the server answers with a found marker and a 4-byte
//! port, or any other byte for "not registered", and closes.

use crate::{DecodeError, wire};

/// A host server, by what the mapper calls it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Service {
    Central,
    Database,
    DataQueue,
    File,
    NetworkPrint,
    RemoteCommand,
    Signon,
}

impl Service {
    /// Every service, sign-on first.
    pub const ALL: [Service; 7] = [
        Service::Signon,
        Service::Central,
        Service::Database,
        Service::DataQueue,
        Service::File,
        Service::NetworkPrint,
        Service::RemoteCommand,
    ];

    /// The mapper's name for the service, with the TLS suffix when `tls`.
    pub fn mapper_name(self, tls: bool) -> String {
        let base = match self {
            Service::Central => wire::SERVICE_CENTRAL,
            Service::Database => wire::SERVICE_DATABASE,
            Service::DataQueue => wire::SERVICE_DATA_QUEUE,
            Service::File => wire::SERVICE_FILE,
            Service::NetworkPrint => wire::SERVICE_NETWORK_PRINT,
            Service::RemoteCommand => wire::SERVICE_REMOTE_COMMAND,
            Service::Signon => wire::SERVICE_SIGNON,
        };
        if tls {
            format!("{base}{}", wire::TLS_SUFFIX)
        } else {
            base.to_owned()
        }
    }

    /// The service a mapper name asks for, and whether it is the TLS port.
    pub fn from_mapper_name(name: &str) -> Option<(Service, bool)> {
        let (base, tls) = match name.strip_suffix(wire::TLS_SUFFIX) {
            Some(b) => (b, true),
            None => (name, false),
        };
        Service::ALL
            .into_iter()
            .find(|s| s.mapper_name(false) == base)
            .map(|s| (s, tls))
    }

    /// The server ID in the service's data stream headers.
    pub fn server_id(self) -> u16 {
        match self {
            Service::Central => wire::SERVER_CENTRAL,
            Service::Database => wire::SERVER_DATABASE,
            Service::DataQueue => wire::SERVER_DATA_QUEUE,
            Service::File => wire::SERVER_FILE,
            Service::NetworkPrint => wire::SERVER_NETWORK_PRINT,
            Service::RemoteCommand => wire::SERVER_REMOTE_COMMAND,
            Service::Signon => wire::SERVER_SIGNON,
        }
    }

    /// A name for the user: "sign-on", "central", ...
    pub fn display_name(self) -> &'static str {
        match self {
            Service::Central => "central",
            Service::Database => "database",
            Service::DataQueue => "data queue",
            Service::File => "file",
            Service::NetworkPrint => "network print",
            Service::RemoteCommand => "remote command",
            Service::Signon => "sign-on",
        }
    }
}

/// What the client sends: the service name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapperRequest {
    pub service: Service,
    pub tls: bool,
}

impl MapperRequest {
    pub fn encode(&self) -> Vec<u8> {
        self.service.mapper_name(self.tls).into_bytes()
    }

    /// The service a request asks for. A name this crate does not know is
    /// `Ok(None)`: the mapper answers "not registered".
    pub fn decode(bytes: &[u8]) -> Result<Option<MapperRequest>, DecodeError> {
        let name =
            std::str::from_utf8(bytes).map_err(|_| DecodeError::Mapper("name is not ASCII"))?;
        Ok(Service::from_mapper_name(name).map(|(service, tls)| MapperRequest { service, tls }))
    }
}

/// What the mapper answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapperReply {
    Port(u16),
    NotRegistered,
}

impl MapperReply {
    /// Bytes in a found reply: the marker and a 4-byte port.
    pub const FOUND_LEN: usize = 5;

    pub fn encode(&self) -> Vec<u8> {
        match self {
            MapperReply::Port(p) => {
                let mut v = vec![wire::MAPPER_FOUND];
                v.extend_from_slice(&u32::from(*p).to_be_bytes());
                v
            }
            MapperReply::NotRegistered => vec![wire::MAPPER_NOT_FOUND],
        }
    }

    /// Reads a reply. The first byte decides: anything but the found
    /// marker is "not registered" (and nothing more need be read).
    pub fn decode(bytes: &[u8]) -> Result<MapperReply, DecodeError> {
        match bytes.first() {
            None => Err(DecodeError::Short { need: 1, got: 0 }),
            Some(&wire::MAPPER_FOUND) => {
                if bytes.len() < Self::FOUND_LEN {
                    return Err(DecodeError::Short {
                        need: Self::FOUND_LEN,
                        got: bytes.len(),
                    });
                }
                let port = u32::from_be_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
                match u16::try_from(port) {
                    Ok(p) if p > 0 => Ok(MapperReply::Port(p)),
                    _ => Err(DecodeError::Mapper("port is not 1 to 65535")),
                }
            }
            Some(_) => Ok(MapperReply::NotRegistered),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_round_trip_for_every_service_and_tls() {
        for s in Service::ALL {
            for tls in [false, true] {
                let r = MapperRequest { service: s, tls };
                assert_eq!(MapperRequest::decode(&r.encode()), Ok(Some(r.clone())));
            }
        }
        let tls = MapperRequest {
            service: Service::Signon,
            tls: true,
        };
        assert_eq!(tls.encode().len(), wire::SERVICE_SIGNON.len() + 2);
        assert_eq!(MapperRequest::decode(b"telnet"), Ok(None));
        assert!(MapperRequest::decode(&[0xFF]).is_err());
    }

    #[test]
    fn replies_round_trip() {
        for r in [
            MapperReply::Port(8476),
            MapperReply::Port(9476),
            MapperReply::NotRegistered,
        ] {
            assert_eq!(MapperReply::decode(&r.encode()), Ok(r));
        }
        assert_eq!(MapperReply::Port(8476).encode(), [0x2B, 0, 0, 0x21, 0x1C]);
        // Any other first byte means not registered.
        assert_eq!(MapperReply::decode(&[0x00]), Ok(MapperReply::NotRegistered));
        assert!(MapperReply::decode(&[0x2B, 0, 0]).is_err());
        assert!(MapperReply::decode(&[0x2B, 0, 1, 0, 0]).is_err());
        assert!(MapperReply::decode(&[0x2B, 0, 0, 0, 0]).is_err());
        assert!(MapperReply::decode(&[]).is_err());
    }

    #[test]
    fn server_ids_are_distinct() {
        let mut ids: Vec<u16> = Service::ALL.iter().map(|s| s.server_id()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), Service::ALL.len());
    }
}
