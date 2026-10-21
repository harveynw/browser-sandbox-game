use server::{admission::random_id, content_loader::WorldContent};
use std::{
    fs,
    path::{Path, PathBuf},
};
struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn copy(source: &Path, target: &Path) {
    fs::create_dir_all(target).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let target = target.join(entry.file_name());
        if path.is_dir() {
            copy(&path, &target);
        } else {
            fs::copy(path, target).unwrap();
        }
    }
}
#[test]
fn runtime_loader_rejects_tampered_missing_oversized_and_traversal_assets() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/client/public/content");
    let scratch = Scratch(std::env::temp_dir().join(random_id("pm-content-test-", 8).unwrap()));
    copy(&source, &scratch.0);
    let original = fs::read(scratch.0.join("slice.json")).unwrap();
    let mut index: serde_json::Value = serde_json::from_slice(&original).unwrap();
    let package = scratch
        .0
        .join("packages")
        .join(index["content_hash"].as_str().unwrap());
    assert!(WorldContent::load(&scratch.0).is_ok());
    index["manifest_url"] = "../manifest.json".into();
    fs::write(scratch.0.join("slice.json"), index.to_string()).unwrap();
    assert!(WorldContent::load(&scratch.0).is_err());
    fs::write(scratch.0.join("slice.json"), original).unwrap();
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(package.join("manifest.json")).unwrap()).unwrap();
    let collision = package.join(manifest["collision_url"].as_str().unwrap());
    let original = fs::read(&collision).unwrap();
    let mut changed = original.clone();
    changed.push(b' ');
    fs::write(&collision, changed).unwrap();
    assert!(WorldContent::load(&scratch.0).is_err());
    fs::write(&collision, vec![b' '; 2 * 1024 * 1024 + 1]).unwrap();
    assert!(WorldContent::load(&scratch.0).is_err());
    fs::remove_file(&collision).unwrap();
    assert!(WorldContent::load(&scratch.0).is_err());
    fs::write(&collision, original).unwrap();
    assert!(WorldContent::load(&scratch.0).is_ok());
}
