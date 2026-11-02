# Changelog

Changes to the public API of the crates other projects depend on by git
(QS/400 pins them to a commit), and removals of crates that were published
here. Breaking changes are marked **Breaking**.

## Unreleased

### qsa-net

- `info!`, beside `debug!`, `trace!` and `warn!`.
- **Breaking:** `TlsOptions` has a new field, `tls13_only`: offer TLS 1.3
  alone (QAP) instead of TLS 1.2 and 1.3 (Telnet).

### hostsvr-proto

- **Breaking:** removed. QS/400 Access talks to QS/400 over QAP
  (`docs/qap-protocol-spec.md`) instead of the host-server protocol. The
  crate stays in git history (last at `c8bff49`); nothing should pin it.

### pwsub

- **Breaking:** removed; last at `a5040f5`. 5250 automatic sign-on uses a
  one-time QAP `TelnetTicket` instead of a password substitute, so the DES
  and SHA-1 forms have no use. Its fuzz target and RFC 4777 vector fixtures
  went with it.
- Before that, in this release: removed `change`, `Change` and the
  `unverified-change` feature (the unverified change-password forms), and
  `Secret::from_vec`.

## Earlier (unreleased)

### pwsub

- First version: `Scheme`, `Seeds`, `Secret` (with `from_vec`),
  `substitute`, `telnet`, `user_id`, `Error`.
- `change` and `Change` are behind the `unverified-change` feature, off by
  default: the protected-password forms are not yet checked against a real
  system.

### hostsvr-proto

- First version: `frame` (`Header`, `Param`, `Frame`, `frame_length`),
  `mapper` (`Service`, `MapperRequest`, `MapperReply`), `message`
  (`HostMessage`, `from_frame`, `push_into`), `rc` (`ReturnCode`,
  `Class`), `wire`, `DecodeError`, `reply_id`.
- `wire::SIGNON_DATASTREAM_LEVEL` is 5: password levels 0 to 3, no level 4.
- The sign-on exchanges were never added.
