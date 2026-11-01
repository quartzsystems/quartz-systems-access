// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The host servers' wire constants: service names, server IDs, request
//! and reply IDs, code points and fixed values.
//!
//! This is the only file in the crate the clean-room check allows to hold
//! vendor-prefixed protocol strings. Other code uses these constants by
//! name.
//!
//! Sources, each read as a specification only (no code was copied or
//! ported):
//!
//! - "JTOpen": the open-source Java toolbox's classes for the data stream
//!   named in each comment, read for field layouts and values.
//! - "port table": the published table of host server service names and
//!   port numbers (the same values as `qsa-config`'s port defaults).
//!
//! Everything here is checked against real systems by the recorded
//! exchanges in `fixtures/` as they are added; until then a value is only
//! as good as its source.

// Server mapper.

/// The server mapper's port. Port table.
pub const MAPPER_PORT: u16 = 449;

/// The first byte of a mapper reply that carries a port. JTOpen
/// `AS400PortMapReplyDS`.
pub const MAPPER_FOUND: u8 = 0x2B;

/// The first byte this crate sends for "not registered" when it is the
/// mapper; a client takes any byte other than [`MAPPER_FOUND`] as that.
/// JTOpen `AS400PortMapReplyDS` (which only tests for the found marker).
pub const MAPPER_NOT_FOUND: u8 = 0x2D;

/// Appended to a service name to ask for its TLS port. JTOpen `PortMapper`.
pub const TLS_SUFFIX: &str = "-s";

/// Central server service name. Port table; JTOpen `AS400` (service names).
pub const SERVICE_CENTRAL: &str = "as-central";
/// Database server service name. Port table.
pub const SERVICE_DATABASE: &str = "as-database";
/// Data queue server service name. Port table.
pub const SERVICE_DATA_QUEUE: &str = "as-dtaq";
/// File server service name. Port table.
pub const SERVICE_FILE: &str = "as-file";
/// Network print server service name. Port table.
pub const SERVICE_NETWORK_PRINT: &str = "as-netprt";
/// Remote command server service name. Port table.
pub const SERVICE_REMOTE_COMMAND: &str = "as-rmtcmd";
/// Sign-on server service name. Port table.
pub const SERVICE_SIGNON: &str = "as-signon";

// Server IDs (header bytes 6 and 7).

/// Central server. JTOpen `NLSExchangeAttrRequest`.
pub const SERVER_CENTRAL: u16 = 0xE000;
/// File server. JTOpen `AS400Server` (service name to server ID).
pub const SERVER_FILE: u16 = 0xE002;
/// Network print server. JTOpen `AS400Server`.
pub const SERVER_NETWORK_PRINT: u16 = 0xE003;
/// Database server. JTOpen `AS400Server`.
pub const SERVER_DATABASE: u16 = 0xE004;
/// Data queue server. JTOpen `AS400Server`.
pub const SERVER_DATA_QUEUE: u16 = 0xE007;
/// Remote command server. JTOpen `AS400Server`.
pub const SERVER_REMOTE_COMMAND: u16 = 0xE008;
/// Sign-on server. JTOpen `SignonExchangeAttributeReq`.
pub const SERVER_SIGNON: u16 = 0xE009;

// Request IDs (header bytes 18 and 19). A reply's ID is its request's
// with the top bit set.

/// Set in a reply's ID. JTOpen (each reply's `hashCode`, e.g. `F003` for
/// `7003`).
pub const REPLY_BIT: u16 = 0x8000;

/// Exchange random seeds, any server but sign-on. JTOpen
/// `AS400XChgRandSeedDS`.
pub const REQ_EXCHANGE_SEEDS: u16 = 0x7001;
/// Start server (authenticate), any server but sign-on. JTOpen
/// `AS400StrSvrDS`.
pub const REQ_START_SERVER: u16 = 0x7002;
/// Sign-on server: exchange attributes. JTOpen `SignonExchangeAttributeReq`.
pub const REQ_SIGNON_EXCHANGE: u16 = 0x7003;
/// Sign-on server: retrieve sign-on information. JTOpen `SignonInfoReq`.
pub const REQ_SIGNON_INFO: u16 = 0x7004;
/// Sign-on server: change password. JTOpen `ChangePasswordReq`.
pub const REQ_CHANGE_PASSWORD: u16 = 0x7005;
/// Central server: exchange attributes (national language support).
/// JTOpen `NLSExchangeAttrRequest`.
pub const REQ_CENTRAL_EXCHANGE: u16 = 0x1301;

// Header and template values.

/// Client attributes (header byte 4) in an exchange of seeds: SHA-1
/// substitutes supported. JTOpen `AS400XChgRandSeedDS` (which also sets
/// bits for features this client does not have).
pub const CLIENT_ATTR_SHA1: u8 = 0x01;
/// Client attributes (header byte 4) in a start-server request: job
/// information wanted in the reply. JTOpen `AS400StrSvrDS`.
pub const CLIENT_ATTR_JOB_INFO: u8 = 0x02;

/// Authentication type (template byte): DES substitute, 8 bytes. JTOpen
/// `SignonInfoReq`, `AS400StrSvrDS`, `ChangePasswordReq`.
pub const AUTH_DES: u8 = 0x01;
/// Authentication type: SHA-1 substitute, 20 bytes. As [`AUTH_DES`].
pub const AUTH_SHA1: u8 = 0x03;

/// The client version this client reports. JTOpen
/// `SignonExchangeAttributeReq`.
pub const CLIENT_VERSION: u32 = 1;
/// The sign-on data stream level this client reports in the sign-on
/// server's exchange of attributes. 5 means: substitutes for password
/// levels 0 to 3 (DES and SHA-1), and no level 4 scheme, which this client
/// does not have (higher levels announce it). QS/400's sign-on server must
/// advertise the same level in its reply, so that neither side expects the
/// level 4 exchange. Unverified against a real system. JTOpen
/// `SignonExchangeAttributeReq` (its note on levels per password level).
pub const SIGNON_DATASTREAM_LEVEL: u16 = 5;
/// From this server level on, the sign-on server can return messages when
/// asked. JTOpen `SignonInfoReq`, `ChangePasswordReq`.
pub const SERVER_LEVEL_MESSAGES: u16 = 5;

/// UTF-16 (CCSID 1200): the client CCSID sent with retrieve sign-on
/// information. JTOpen `SignonInfoReq`.
pub const CCSID_UTF16: u32 = 1200;
/// UCS-2 (CCSID 13488): passwords in a change-password request at levels 2
/// and 3, and the central server's client CCSID. JTOpen
/// `ChangePasswordReq`, `NLSExchangeAttrRequest`.
pub const CCSID_UCS2: u32 = 13488;
/// The client version sent to the central server (not used by it). JTOpen
/// `NLSExchangeAttrRequest`.
pub const CENTRAL_CLIENT_VERSION: u32 = 0x0310;

// Code points (LL/CP parameters).

/// Client or server version, u32. JTOpen `SignonExchangeAttributeReq`/`Rep`.
pub const CP_VERSION: u16 = 0x1101;
/// Client or server data stream level, u16. As [`CP_VERSION`].
pub const CP_LEVEL: u16 = 0x1102;
/// Client or server random seed, 8 bytes. As [`CP_VERSION`].
pub const CP_SEED: u16 = 0x1103;
/// User ID: 10 EBCDIC bytes in a request; CCSID (u32) then the name in a
/// reply. JTOpen `SignonInfoReq`, `SignonInfoRep`, `AS400StrSvrReplyDS`.
pub const CP_USER_ID: u16 = 0x1104;
/// Password substitute. JTOpen `SignonInfoReq`, `AS400StrSvrDS`.
pub const CP_PASSWORD: u16 = 0x1105;
/// Current sign-on date and time. JTOpen `SignonInfoRep`.
pub const CP_CURRENT_SIGNON: u16 = 0x1106;
/// Last sign-on date and time. JTOpen `SignonInfoRep`.
pub const CP_LAST_SIGNON: u16 = 0x1107;
/// Password expiration date. JTOpen `SignonInfoRep`.
pub const CP_PASSWORD_EXPIRES: u16 = 0x1108;
/// Protected old password. JTOpen `ChangePasswordReq`.
pub const CP_PROTECTED_OLD: u16 = 0x110C;
/// Protected new password. JTOpen `ChangePasswordReq`.
pub const CP_PROTECTED_NEW: u16 = 0x110D;
/// Client CCSID, u32. JTOpen `SignonInfoReq`.
pub const CP_CLIENT_CCSID: u16 = 0x1113;
/// Server CCSID, u32. JTOpen `SignonInfoRep`.
pub const CP_SERVER_CCSID: u16 = 0x1114;
/// Password level, one byte. JTOpen `SignonExchangeAttributeRep`.
pub const CP_PASSWORD_LEVEL: u16 = 0x1119;
/// Old password length in bytes, u32 (levels 2 and 3). JTOpen
/// `ChangePasswordReq`.
pub const CP_OLD_LENGTH: u16 = 0x111C;
/// New password length in bytes, u32 (levels 2 and 3). JTOpen
/// `ChangePasswordReq`.
pub const CP_NEW_LENGTH: u16 = 0x111D;
/// CCSID of the passwords, u32 (levels 2 and 3). JTOpen `ChangePasswordReq`.
pub const CP_PASSWORD_CCSID: u16 = 0x111E;
/// Server job name: CCSID (u32) then the name. JTOpen
/// `SignonExchangeAttributeRep`.
pub const CP_JOB_NAME: u16 = 0x111F;
/// Return messages: one byte, 1 = yes. JTOpen `SignonInfoReq`.
pub const CP_RETURN_MESSAGES: u16 = 0x1128;
/// Number of messages that follow, u16. JTOpen `AS400ImplRemote`
/// (message parsing).
pub const CP_MESSAGE_COUNT: u16 = 0x112A;
/// One message. JTOpen `AS400ImplRemote` (message parsing).
pub const CP_MESSAGE: u16 = 0x112B;
/// Password expiration warning, days, u32. JTOpen `SignonInfoRep`.
pub const CP_EXPIRY_WARNING: u16 = 0x112C;
/// Additional authentication factor indicator, one byte (read, never
/// used by this client). JTOpen `SignonExchangeAttributeRep`.
pub const CP_AAF_INDICATOR: u16 = 0x112E;
/// The central server's CCSID list, in its exchange-attributes reply.
/// JTOpen `NLSExchangeAttrReply`.
pub const CP_CCSID_LIST: u16 = 0x0008;
