# Code page data

`cpNNN.txt` maps each byte of a single-byte EBCDIC CCSID to a Unicode code
point. `tn5250/build.rs` compiles them into `tn5250::codepage`, and a unit test
proves each table round-trips all 256 bytes.

The data was produced on Windows from the platform's EBCDIC code page
definitions (37, 1140, 20273 for 273, 20285 for 285, 20297 for 297, 500),
checked to be a one-to-one mapping of all 256 bytes, and checked against the
positions where these CCSIDs differ from each other. No emulator's tables
were used.
