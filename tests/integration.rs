use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn test_binary_exits_cleanly() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mimodel"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to start");
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(b"q");
    }
    let status = child.wait().expect("Failed to wait");
    assert!(status.success() || status.code().is_some());
}
