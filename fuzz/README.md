# Fuzzing `tn5250` and `scs`

Four `cargo fuzz` targets (libFuzzer; needs a nightly toolchain and
`cargo install cargo-fuzz`). The crate is its own workspace and is not built
by `cargo build --workspace`.

| Target | Input | Checks |
|---|---|---|
| `telnet_decoder` | a byte stream, fed to the Telnet layer and the TN5250E negotiator in pieces whose size the first byte sets | no panic; escaping then decoding returns the data; the record, start-up response, printer record, `NEW-ENVIRON`, save-screen, recording and certificate readers never panic |
| `stream_parser` | a flags byte, then records (opcode, 16-bit length, data) | no panic; after every record the screen is consistent (size, cursor, fields in order and on the screen) |
| `parser_input` | a flags byte, then records, keys, characters and pastes in any order (format in the target) | as above, through `Terminal`, with the keyboard in play |
| `scs_parser` | an options byte (code page, paper, device font), then one SCS print job | no panic; no more pages than bytes; every run has text, a line, a column and a height; the text rendering has one form feed between each two pages |

For the first three, the flags byte picks 24×80 or 27×132 (bit 0) and the
CCSID (bits 1 to 3).

## Running

```
cd fuzz
cargo +nightly fuzz run stream_parser -- -max_total_time=3600 -fork=6
```

Crashes land in `fuzz/artifacts/<target>/`; reproduce one with
`cargo +nightly fuzz run <target> fuzz/artifacts/<target>/crash-...`, fix it,
and add the input to the unit tests of the code that failed.

Without a nightly toolchain on the machine, run it in a container (Docker):

```
docker run --rm -v "$PWD:/src" -w /src/fuzz rustlang/rust:nightly   sh -c 'cargo install cargo-fuzz && cargo fuzz run scs_parser -- -max_total_time=3600'
```

## Seeds

`corpus/<target>/seed-*` are committed; everything else the fuzzer adds to
`corpus/` is not. The `scs_parser` seeds are the RFC 4777 example print
record's SCS and two jobs that use most controls. The others come from the
replay fixtures:

```
cargo run -p tn5250 --example fuzz_seeds
```

Run it again after recording new fixtures.
