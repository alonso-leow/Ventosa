//! The auto review's pure checks (ffprobe/ffmpeg parsing, severity routing, loop guard) are unit tested in Python.
#[test]
fn review_checks_python_tests_pass() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("engine");
    let Ok(out) = std::process::Command::new("python3").arg("-I").arg(dir.join("test_review.py")).output() else { return };
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}
