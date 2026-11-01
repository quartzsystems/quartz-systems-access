// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

use std::io::{Read, Write};
use std::net::TcpListener;

use super::*;
use crate::negotiate::{ESC, IS, Outcome, SEND, USERVAR, VALUE, VAR};
use crate::telnet::{BINARY, DO, EOR_OPTION, IAC, NEW_ENVIRON, SB, SE, TERMINAL_TYPE, WILL, codec};

fn hex(s: &str) -> Vec<u8> {
    let s: String = s.split_whitespace().collect();
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn rec(bytes: &[u8]) -> Record {
    Record::parse(bytes).unwrap()
}

/// RFC 4777 section 11.1, figure 4: a print record, first and last of
/// chain.
const FIGURE_4: &str = "0085 12A0 0101 0A 1800 01 000000000000
    34C4012BD20345FF2BD2044C00022BD2040D00002BD20A8501010201030204022BD20309022BD2061100014A
    402BD20601010000012BD306F60000FFFF2BD20A48000001000000010100
    2BD10705000B0090012BD2044900F02BD206404A403DE02BD2041500F034
    C4012BD10381FF002BC8034001";

/// Section 11.3, figure 6: the null print record that ends a job.
const FIGURE_6: &str = "0011 12A0 0101 0A 0800 01 000000000000 00";

#[test]
fn print_complete_is_figure_5() {
    assert_eq!(
        Response::Complete.encode(op::PRINT),
        hex("000A 12A0 0102 04 0000 01")
    );
}

#[test]
fn error_responses_carry_their_diagnostic_in_the_header() {
    let r = Response::Intervention(Condition::NotReady).encode(op::PRINT);
    assert_eq!(r, hex("000F 12A0 0102 09 4000 01 C900030251"));
    // The header length counts the diagnostic, so the record parses with
    // no data after it.
    let p = rec(&r);
    assert_eq!(
        (p.reserved, p.flags, p.opcode),
        (flow::TO_SERVER, flag::INTERVENTION, op::PRINT)
    );
    assert!(p.data.is_empty());
    assert_eq!(
        Response::Intervention(Condition::EndOfForms).encode(op::PRINT)[10..],
        [0xC9, 0x00, 0x03, 0x02, 0x50]
    );
    assert_eq!(
        Response::Intervention(Condition::DataStream(2)).encode(op::PRINT)[14],
        0x67
    );
    assert_eq!(
        Response::Ready.encode(op::PRINT),
        hex("000F 12A0 0102 09 2000 01 C900000002")
    );
    assert_eq!(
        Response::Negative(Negative::InvalidCommand).encode(7),
        hex("000E 12A0 0102 08 8000 07 08110228")
    );
    assert_eq!(
        Response::Negative(Negative::Cancel).encode(op::PRINT)[10..],
        [0x08, 0x11, 0x02, 0x00]
    );
}

#[test]
fn a_print_record_starts_a_job_and_the_null_record_ends_it() {
    let mut p = Printer::new();
    let now = Instant::now();
    let r = rec(&hex(FIGURE_4));
    let Handled::Caller { events, opcode } = p.record(&r, now) else {
        panic!("print records are answered by the caller");
    };
    assert_eq!(opcode, op::PRINT);
    assert_eq!(
        events[0],
        PrintEvent::JobStarted {
            job: 1,
            transformed: false
        }
    );
    let PrintEvent::Data { job: 1, bytes } = &events[1] else {
        panic!("{events:?}")
    };
    // The SCS data starts after the ten-byte header.
    assert_eq!(&bytes[..3], &[0x34, 0xC4, 0x01]);
    assert_eq!(bytes.len(), 0x85 - 16);
    assert_eq!(p.current_job(), Some(1));

    // More data joins the job.
    let Handled::Caller { events, .. } = p.record(&r, now) else {
        panic!()
    };
    assert!(matches!(&events[..], [PrintEvent::Data { job: 1, .. }]));

    let Handled::Caller { events, opcode } = p.record(&rec(&hex(FIGURE_6)), now) else {
        panic!()
    };
    assert_eq!(opcode, op::PRINT);
    assert_eq!(
        events,
        vec![PrintEvent::JobEnded {
            job: 1,
            end: JobEnd::NullRecord
        }]
    );
    assert_eq!(p.current_job(), None);

    // The next data is a new job.
    let Handled::Caller { events, .. } = p.record(&r, now) else {
        panic!()
    };
    assert!(matches!(events[0], PrintEvent::JobStarted { job: 2, .. }));
}

#[test]
fn a_null_record_without_its_null_byte_still_ends_the_job() {
    let mut p = Printer::new();
    let now = Instant::now();
    p.record(&rec(&hex(FIGURE_4)), now);
    let Handled::Caller { events, .. } =
        p.record(&rec(&hex("0010 12A0 0101 0A 0800 01 000000000000")), now)
    else {
        panic!()
    };
    assert!(matches!(events[..], [PrintEvent::JobEnded { job: 1, .. }]));
    // A null record with no job open is still answered, and ends nothing.
    let Handled::Caller { events, .. } = p.record(&rec(&hex(FIGURE_6)), now) else {
        panic!()
    };
    assert!(events.is_empty());
}

#[test]
fn transformed_output_is_recognised() {
    // Section 12: the first print record of a job printed with host print
    // transform: ASCII transparency around printer commands.
    let first = hex("00DF12A001010A18 0001000000000000
         03CD1B451B283130 551B287330703130");
    let mut p = Printer::new();
    let Handled::Caller { events, .. } = p.record(&rec(&first), Instant::now()) else {
        panic!()
    };
    assert_eq!(
        events[0],
        PrintEvent::JobStarted {
            job: 1,
            transformed: true
        }
    );
}

#[test]
fn clear_print_buffers_drops_the_job() {
    let mut p = Printer::new();
    let now = Instant::now();
    p.record(&rec(&hex(FIGURE_4)), now);
    let clear = hex("0010 12A0 0101 0A 0000 02 000000000000");
    assert_eq!(
        p.record(&rec(&clear), now),
        Handled::Caller {
            events: vec![PrintEvent::Cleared { job: Some(1) }],
            opcode: op::CLEAR
        }
    );
    assert_eq!(p.current_job(), None);
}

#[test]
fn other_operations_are_rejected_by_the_protocol() {
    let mut p = Printer::new();
    let r = hex("0011 12A0 0101 0A 0000 09 000000000000 40");
    assert_eq!(
        p.record(&rec(&r), Instant::now()),
        Handled::Now {
            response: Response::Negative(Negative::InvalidCommand),
            opcode: 9
        }
    );
    assert_eq!(p.current_job(), None);
}

#[test]
fn idle_closes_a_job_only_when_the_server_gives_no_boundaries() {
    let t0 = Instant::now();
    let ten = Duration::from_secs(10);
    let mut p = Printer::new();
    p.record(&rec(&hex(FIGURE_4)), t0);
    assert_eq!(p.idle(t0 + Duration::from_secs(9), ten), None);
    // Data keeps the job open.
    p.record(&rec(&hex(FIGURE_4)), t0 + Duration::from_secs(9));
    assert_eq!(p.idle(t0 + Duration::from_secs(15), ten), None);
    assert_eq!(
        p.idle(t0 + Duration::from_secs(19), ten),
        Some(PrintEvent::JobEnded {
            job: 1,
            end: JobEnd::Idle
        })
    );
    assert_eq!(p.idle(t0 + Duration::from_secs(60), ten), None);

    // Once a null record has been seen, idle time never ends a job.
    let mut p = Printer::new();
    p.record(&rec(&hex(FIGURE_4)), t0);
    p.record(&rec(&hex(FIGURE_6)), t0);
    p.record(&rec(&hex(FIGURE_4)), t0);
    assert_eq!(p.idle(t0 + Duration::from_secs(3600), ten), None);
    assert_eq!(p.current_job(), Some(2));
}

#[test]
fn no_record_shape_panics() {
    let mut p = Printer::new();
    let now = Instant::now();
    for opcode in 0..=255u8 {
        for data in [&[][..], &[0][..], &[3, 1, 0x41][..], &[0xFF; 40][..]] {
            let mut r = Record::new(opcode, data.to_vec());
            r.reserved = flow::FROM_SERVER;
            let _ = p.record(&r, now);
            let _ = p.idle(now + Duration::from_secs(30), Duration::from_secs(10));
        }
    }
}

/// The environment answer, in the order sent.
fn environ_answer(options: &Options) -> Vec<u8> {
    let mut want = vec![IAC, SB, NEW_ENVIRON, IS];
    for (n, v) in options.request().user_vars() {
        want.push(USERVAR);
        want.extend_from_slice(n.as_bytes());
        want.push(VALUE);
        for b in v {
            if b <= USERVAR {
                want.push(ESC);
            }
            want.push(b);
            if b == IAC {
                want.push(IAC);
            }
        }
    }
    want.extend_from_slice(&[IAC, SE]);
    want
}

#[test]
fn printer_request_has_the_section_8_variables_and_transform_off() {
    let mut o = Options::new(" prt01 ", 37);
    o.message_queue = "prtmsgq".into();
    o.message_queue_library = String::new();
    o.paper_source_1 = Paper::A4;
    let r = o.request();
    assert_eq!(r.terminal_type, wire::TT_PRINTER_SBCS);
    assert_eq!(r.device_name.as_deref(), Some("PRT01"));
    let vars = r.user_vars();
    let get = |n: &str| vars.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone());
    assert_eq!(get("DEVNAME"), Some(b"PRT01".to_vec()));
    assert_eq!(get(wire::MESSAGE_QUEUE_NAME), Some(b"PRTMSGQ".to_vec()));
    // An empty library falls back to the default.
    assert_eq!(get(wire::MESSAGE_QUEUE_LIBRARY), Some(b"QSYS".to_vec()));
    assert_eq!(get(wire::FONT), Some(b"11".to_vec()));
    assert_eq!(get(wire::FORM_FEED), Some(b"C".to_vec()));
    assert_eq!(get(wire::TRANSFORM), Some(b"0".to_vec()));
    assert_eq!(get(wire::PAPER_SOURCE_1), Some(vec![0x04]));
    assert_eq!(get(wire::PAPER_SOURCE_2), Some(vec![0xFF]));
    assert_eq!(get(wire::ENVELOPE), Some(vec![0xFF]));
    assert_eq!(get(wire::ASCII_899), Some(b"0".to_vec()));
    assert_eq!(get(wire::MANUFACTURER_TYPE_MODEL), None);
    assert_eq!(get(wire::PASSWORD_SUBSTITUTE), None);
}

#[test]
fn binary_values_are_escaped_on_the_wire() {
    // RFC 4777 section 9: paper source 1 of 01 needs ESC; envelope FF is
    // doubled as a Telnet IAC.
    let mut o = Options::new("PCPRINTER", 37);
    o.paper_source_1 = Paper::Letter;
    let mut n = negotiate::Negotiator::new(o.request());
    n.receive(&[IAC, DO, NEW_ENVIRON]);
    n.take_output();
    n.receive(&[IAC, SB, NEW_ENVIRON, SEND, VAR, USERVAR, IAC, SE]);
    let out = n.take_output();
    assert_eq!(out, environ_answer(&o));
    let mut src1 = wire::PAPER_SOURCE_1.as_bytes().to_vec();
    src1.extend_from_slice(&[VALUE, ESC, 0x01]);
    assert!(out.windows(src1.len()).any(|w| w == src1));
    let mut env = wire::ENVELOPE.as_bytes().to_vec();
    env.extend_from_slice(&[VALUE, 0xFF, 0xFF]);
    assert!(out.windows(env.len()).any(|w| w == env));
}

/// A scripted server: the RFC 4777 section 12 exchange, shortened. It
/// checks the client's terminal type and environment, sends a start-up
/// response, two print records and the null record, and reads one print
/// complete for each.
#[test]
fn printer_session_over_tcp() {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let options = Options::new("DUMMYPRT", 37);
    let expected_env = environ_answer(&options);
    let server = std::thread::spawn(move || {
        let (mut s, _) = l.accept().unwrap();
        let mut got = Vec::new();
        let mut buf = [0u8; 1024];
        let mut read_until = |s: &mut std::net::TcpStream, got: &mut Vec<u8>, want: &[u8]| {
            while !got.windows(want.len()).any(|w| w == want) {
                let n = s.read(&mut buf).unwrap();
                assert!(n > 0, "client closed early; got {got:02X?}");
                got.extend_from_slice(&buf[..n]);
            }
        };
        s.write_all(&[IAC, DO, NEW_ENVIRON, IAC, DO, TERMINAL_TYPE])
            .unwrap();
        s.write_all(&[IAC, SB, NEW_ENVIRON, SEND, VAR, USERVAR, IAC, SE])
            .unwrap();
        read_until(&mut s, &mut got, &expected_env);
        s.write_all(&[IAC, SB, TERMINAL_TYPE, SEND, IAC, SE])
            .unwrap();
        let mut tt = vec![IAC, SB, TERMINAL_TYPE, IS];
        tt.extend_from_slice(wire::TT_PRINTER_SBCS.as_bytes());
        tt.extend_from_slice(&[IAC, SE]);
        read_until(&mut s, &mut got, &tt);
        s.write_all(&[
            IAC, DO, EOR_OPTION, IAC, WILL, EOR_OPTION, IAC, DO, BINARY, IAC, WILL, BINARY,
        ])
        .unwrap();
        let startup = hex(
            "004912A090000560060020C0003D0000C9F9F0F2C5D3C3D9E3D7F0F6C4E4D4D4
             E8D7D9E340400000000000000000000000000000000000000000000000000000
             000000000000000000",
        );
        s.write_all(&codec::encode_record(&startup)).unwrap();
        let complete = codec::encode_record(&Response::Complete.encode(op::PRINT));
        for r in [FIGURE_4, FIGURE_4, FIGURE_6] {
            s.write_all(&codec::encode_record(&hex(r))).unwrap();
            got.clear();
            read_until(&mut s, &mut got, &complete);
        }
        std::thread::sleep(Duration::from_millis(100));
    });

    let target = Target {
        host: "127.0.0.1".into(),
        port,
        tls: None,
    };
    let mut sess = PrinterSession::connect(&target, &options).unwrap();
    let mut events = Vec::new();
    let end = loop {
        match sess.poll(Duration::from_millis(50)) {
            Ok(ev) => {
                for e in ev {
                    if matches!(e, Event::Print(_)) {
                        sess.respond(Response::Complete).unwrap();
                    }
                    events.push(e);
                }
            }
            Err(e) => break e,
        }
    };
    assert!(matches!(end, SessionError::Closed), "{end:?}");
    server.join().unwrap();

    assert_eq!(events[0], Event::Ready(Mode::Enhanced));
    let Event::Startup(s) = &events[1] else {
        panic!("{events:?}")
    };
    assert_eq!(s.outcome(), Outcome::Started);
    assert_eq!(s.device.as_deref(), Some("DUMMYPRT"));
    let print: Vec<&Vec<PrintEvent>> = events
        .iter()
        .filter_map(|e| match e {
            Event::Print(p) => Some(p),
            _ => None,
        })
        .collect();
    assert_eq!(print.len(), 3);
    assert!(matches!(
        print[0][0],
        PrintEvent::JobStarted {
            job: 1,
            transformed: false
        }
    ));
    assert!(matches!(print[1][..], [PrintEvent::Data { job: 1, .. }]));
    assert_eq!(
        print[2][..],
        [PrintEvent::JobEnded {
            job: 1,
            end: JobEnd::NullRecord
        }]
    );
}

#[test]
fn a_response_with_nothing_to_answer_is_not_sent() {
    let (a, _b) = loopback();
    let mut sess = PrinterSession::over(Box::new(a), "127.0.0.1", &Options::new("P", 37));
    // Nothing is waiting for an answer: no bytes go out, and no error.
    sess.respond(Response::Complete).unwrap();
}

fn loopback() -> (crate::telnet::transport::Tcp, std::net::TcpStream) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let c =
        crate::telnet::transport::Tcp::connect("127.0.0.1", port, Duration::from_secs(5)).unwrap();
    let (s, _) = l.accept().unwrap();
    (c, s)
}
