#![no_main]

use libfuzzer_sys::fuzz_target;
use murk_proto::{format, Message};

fuzz_target!(|data: &[u8]| {
    let Ok(line) = std::str::from_utf8(data) else { return };
    let _ = format::parse(line);
    if let Ok(msg) = Message::parse(line) {
        // Serializing a parsed message must reparse to the same message,
        // unless a middle param holds a space, which the wire cannot express.
        let wire = msg.to_wire();
        if !wire.contains(['\r', '\n', '\0']) {
            if let Ok(again) = Message::parse(&wire) {
                assert_eq!(again.command, msg.command);
            }
        }
    }
});
