use kerbaloc_core::doctor::{backup_polluted, detect_pollution};
use std::fs;

#[test]
fn detects_korean_in_en_us_and_backs_up() {
    let d = tempfile::tempdir().unwrap();
    let gd = d.path().join("GameData");
    fs::create_dir_all(gd.join("A/Localization")).unwrap();
    fs::write(
        gd.join("A/Localization/en-us.cfg"),
        "Localization\n{\n\ten-us\n\t{\n\t\t#a = 안녕하세요\n\t}\n}\n",
    )
    .unwrap();
    fs::create_dir_all(gd.join("B/Localization")).unwrap();
    fs::write(
        gd.join("B/Localization/en-us.cfg"),
        "Localization\n{\n\ten-us\n\t{\n\t\t#a = Hello\n\t}\n}\n",
    )
    .unwrap();

    let p = detect_pollution(d.path());
    assert_eq!(p.len(), 1);
    assert!(p[0].path.to_string_lossy().contains("A"));
    assert_eq!(p[0].korean_values, 1);

    let zip_path = d.path().join("backup.zip");
    let n = backup_polluted(d.path(), &zip_path, &p).unwrap();
    assert_eq!(n, 1);
    let f = fs::File::open(&zip_path).unwrap();
    let mut z = zip::ZipArchive::new(f).unwrap();
    assert_eq!(z.len(), 1);
    assert!(z.by_index(0).unwrap().name().contains("en-us.cfg"));
}
