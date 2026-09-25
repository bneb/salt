use super::*;
use std::fs;

#[test]
fn test_cmd_publish_creates_archive() {
    let tmp_home = crate::test_support::temp_path("sp_test_cmd_publish_home");
    let project_dir = crate::test_support::temp_path("sp_test_cmd_publish_project");
    let _ = fs::remove_dir_all(&tmp_home);
    let _ = fs::remove_dir_all(&project_dir);
    fs::create_dir_all(&tmp_home).unwrap();
    fs::create_dir_all(project_dir.join("src")).unwrap();
    fs::write(
        project_dir.join("salt.toml"),
        "[package]\nname = \"pubtest\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        project_dir.join("src/main.salt"),
        "package main\nfn main() -> i32 { return 0; }\n",
    )
    .unwrap();

    let guard = crate::test_support::HomeGuard::new(&tmp_home);

    cmd_publish(&project_dir).expect("cmd_publish should succeed");

    let archive = tmp_home.join(".salt/publish/pubtest-0.1.0.tar.gz");
    assert!(
        archive.exists(),
        "expected archive at {}",
        archive.display()
    );

    drop(guard);
    let _ = fs::remove_dir_all(&tmp_home);
    let _ = fs::remove_dir_all(&project_dir);
}
