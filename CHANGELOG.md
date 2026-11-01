# Changelog

Changes to the public API of the crates other projects depend on by git:
`hostsvr-proto` and `pwsub` (QS/400 pins them to a commit). Breaking
changes are marked **Breaking**.

## Unreleased

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
- The sign-on exchanges are not in this version; see
  `hostsvr-proto/SIGNON-SPEC.md`.
