#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        // Fuzz the ara.toml parser — panics or hangs are bugs
        let _ = ara_manifest::parser::parse(s);
        // Also fuzz package.json parsing
        let _ = ara_manifest::package_json::parse_package_json(s);
    }
});
