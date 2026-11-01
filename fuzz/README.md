# Fuzzing `tn5250`

Three `cargo fuzz` targets (libFuzzer; needs a nightly toolchain and
`cargo install cargo-fuzz`). The crate is its own workspace and is not built
by `cargo build --workspace`.

| Target | Input | Checks |
|---|---|---|
| `telnet_decoder` | a byte stream, fed to the Telnet layer and the TN5250E negotiator in pieces whose size the first byte sets | no panic; escaping then decoding returns the data; the record, start-up response, `NEW-ENVIRON`, save-screen, recording and certificate readers never panic |
| `stream_parser` | a flags byte, then records (opcode, 16-bit length, data) | no panic; after every record the screen is consistent (size, cursor, fields in order and on the screen) |
| `parser_input` | a flags byte, then records, keys, characters and pastes in any order (format in the target) | as above, through `Terminal`, with the keyboard in play |

The flags byte picks 24×80 or 27×132 (bit 0) and the CCSID (bits 1 to 3).

## Running

```
cd fuzz
cargo +nightly fuzz run stream_parser -- -max_total_time=3600 -fork=6
```

Crashes land in `fuzz/artifacts/<target>/`; reproduce one with
`cargo +nightly fuzz run <target> fuzz/artifacts/<target>/crash-...`, fix it,
and add the input to the unit tests of the code that failed.

## Seeds

`corpus/<target>/seed-*` are committed; everything else the fuzzer adds to
`corpus/` is not. The seeds come from the replay fixtures:

```
cargo run -p tn5250 --example fuzz_seeds
```

Run it again after recording new fixtures.
