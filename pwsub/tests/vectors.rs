// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Every vector in `fixtures/pwsub/*.vec`: the RFC 4777 examples, and the
//! exchanges recorded from real systems with throwaway test profiles (see
//! `fixtures/README.md`). A vector holds `key = value` lines:
//!
//! - `kind`: `substitute`, `telnet` or `change`;
//! - `level`, `user`, `password`, `server_seed`, `client_seed` (hex);
//! - `substitute` (hex), the expected substitute;
//! - for `change`: `new_password`, and the expected `old_protected` and
//!   `new_protected` (hex).
//!
//! Recorded vectors also say `accepted = yes`: the system accepted what
//! the client sent, which is what makes them worth having.

use std::collections::BTreeMap;
use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures/pwsub")
}

fn hex(s: &str) -> Vec<u8> {
    let s: String = s.split_whitespace().collect();
    assert!(s.len().is_multiple_of(2), "odd hex {s}");
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect()
}

fn seed(s: &str) -> [u8; 8] {
    hex(s).try_into().expect("eight-byte seed")
}

fn read(path: &PathBuf) -> BTreeMap<String, String> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let (k, v) = l.split_once('=').expect("key = value");
            (k.trim().to_owned(), v.trim().to_owned())
        })
        .collect()
}

#[test]
fn every_vector_matches() {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir())
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "vec"))
        .collect();
    paths.sort();
    assert!(paths.len() >= 3, "the RFC vectors at least: {paths:?}");
    for path in &paths {
        let v = read(path);
        let get = |k: &str| {
            v.get(k)
                .unwrap_or_else(|| panic!("{}: no {k}", path.display()))
                .as_str()
        };
        let level: u8 = get("level").parse().unwrap();
        let seeds = pwsub::Seeds {
            server: seed(get("server_seed")),
            client: seed(get("client_seed")),
        };
        let name = path.file_name().unwrap().to_string_lossy();
        match get("kind") {
            "substitute" => {
                let s = pwsub::substitute(level, get("user"), get("password"), &seeds).unwrap();
                assert_eq!(s.expose(), hex(get("substitute")), "{name}");
            }
            "telnet" => {
                let t = pwsub::telnet(level, get("user"), get("password"), &seeds).unwrap();
                assert_eq!(t.client_seed, seeds.client, "{name}");
                assert_eq!(t.substitute.expose(), hex(get("substitute")), "{name}");
            }
            #[cfg(feature = "unverified-change")]
            "change" => {
                let c = pwsub::change(
                    level,
                    get("user"),
                    get("password"),
                    get("new_password"),
                    &seeds,
                )
                .unwrap();
                assert_eq!(c.substitute.expose(), hex(get("substitute")), "{name}");
                assert_eq!(
                    c.old_protected.expose(),
                    hex(get("old_protected")),
                    "{name}"
                );
                assert_eq!(
                    c.new_protected.expose(),
                    hex(get("new_protected")),
                    "{name}"
                );
            }
            #[cfg(not(feature = "unverified-change"))]
            "change" => eprintln!("{name}: change vectors need the unverified-change feature"),
            k => panic!("{name}: unknown kind {k}"),
        }
    }
}
