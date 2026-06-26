#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // The scanner only processes UTF-8 content, and it's bounded by
    // MAX_FILE_SIZE (1 MB). We create a small temp dir with a single
    // JS file and run the full analysis pipeline.
    if data.len() > 1_048_576 {
        return;
    }
    let dir = match tempfile::TempDir::new() {
        Ok(d) => d,
        Err(_) => return,
    };
    let js_path = dir.path().join("fuzz.js");
    let _ = std::fs::write(&js_path, data);
    let _ = ara_analysis::analyzer::analyze_package(dir.path());
});
