use kerbaloc_core::game::{read_language, set_language};
use std::fs;

const BUILDID: &str = "[config]\r\nbuild id = 03190\r\n2022.12.12 at 14:02:28 PST\r\nBranch: master\r\nlanguage = en-us\r\ndistribution name = Steam\r\n";

#[test]
fn read_and_switch_language_preserving_other_lines() {
    let d = tempfile::tempdir().unwrap();
    fs::write(d.path().join("buildID64.txt"), BUILDID).unwrap();
    assert_eq!(read_language(d.path()).as_deref(), Some("en-us"));
    set_language(d.path(), "ko").unwrap();
    assert_eq!(read_language(d.path()).as_deref(), Some("ko"));
    let text = fs::read_to_string(d.path().join("buildID64.txt")).unwrap();
    assert!(text.contains("build id = 03190"));
    assert!(text.contains("distribution name = Steam"));
    assert!(text.contains("\r\n"), "CRLF 보존");
    set_language(d.path(), "en-us").unwrap();
    assert_eq!(read_language(d.path()).as_deref(), Some("en-us"));
}

#[test]
fn missing_file_returns_none() {
    let d = tempfile::tempdir().unwrap();
    assert!(read_language(d.path()).is_none());
}
