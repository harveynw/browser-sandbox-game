use protocol::{ClientEnvelope, ServerEnvelope, contract, fixtures};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

type Files = BTreeMap<PathBuf, Vec<u8>>;
type TaskResult<T> = Result<T, String>;
const TYPES_ROOT: &str = "packages/protocol/src/generated";
const FIXTURE_ROOT: &str = "tests/fixtures/protocol";

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn json_bytes(value: &serde_json::Value) -> TaskResult<Vec<u8>> {
    let mut result = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    result.push(b'\n');
    Ok(result)
}

fn artifacts() -> TaskResult<Files> {
    let positive = fixtures::positive().map_err(|e| e.to_string())?;
    let mut all = positive.clone();
    all.extend(fixtures::negative(&positive).map_err(|e| e.to_string())?);
    for fixture in &all {
        if fixture.valid != fixtures::decoded_fixture(fixture).is_ok() {
            return Err(format!("invalid reference fixture: {}", fixture.name));
        }
    }
    let sizes: BTreeMap<_, _> = positive
        .iter()
        .filter(|f| f.name.starts_with("max_"))
        .map(|f| (f.name.clone(), f.hex.len() / 2))
        .collect();
    let mut files = Files::new();
    files.insert(
        format!("{TYPES_ROOT}/types.ts").into(),
        contract::typescript().into_bytes(),
    );
    files.insert(
        format!("{TYPES_ROOT}/client.schema.json").into(),
        json_bytes(&contract::schema::<ClientEnvelope>())?,
    );
    files.insert(
        format!("{TYPES_ROOT}/server.schema.json").into(),
        json_bytes(&contract::schema::<ServerEnvelope>())?,
    );
    files.insert(
        format!("{TYPES_ROOT}/limits.json").into(),
        json_bytes(&contract::limits())?,
    );
    files.insert(
        format!("{FIXTURE_ROOT}/manifest.json").into(),
        json_bytes(&serde_json::json!({"version":1,"fixtures":all,"rust_encoded_sizes":sizes}))?,
    );
    Ok(files)
}

fn write_files(root: &Path, files: &Files) -> TaskResult<()> {
    for (relative, bytes) in files {
        let file = root.join(relative);
        fs::create_dir_all(file.parent().ok_or("missing parent")?).map_err(|e| e.to_string())?;
        fs::write(&file, bytes).map_err(|e| format!("{}: {e}", file.display()))?;
    }
    Ok(())
}

fn collect_files(root: &Path, directory: &Path, files: &mut Vec<PathBuf>) -> TaskResult<()> {
    if !directory.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(directory).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            collect_files(root, &entry.path(), files)?;
        } else {
            files.push(
                entry
                    .path()
                    .strip_prefix(root)
                    .map_err(|e| e.to_string())?
                    .into(),
            );
        }
    }
    Ok(())
}

fn compare(root: &Path, regenerated: &Path, expected: &Files) -> TaskResult<()> {
    let mut actual = Vec::new();
    for directory in [TYPES_ROOT, FIXTURE_ROOT] {
        collect_files(root, &root.join(directory), &mut actual)?;
    }
    actual.sort();
    let expected_paths: Vec<_> = expected.keys().cloned().collect();
    if actual != expected_paths {
        return Err("Generated file set drift. Run npm run protocol:generate.".into());
    }
    for relative in expected.keys() {
        let current = fs::read(root.join(relative)).map_err(|e| e.to_string())?;
        let fresh = fs::read(regenerated.join(relative)).map_err(|e| e.to_string())?;
        if current != fresh {
            return Err(format!(
                "Generated file drift: {}. Run npm run protocol:generate.",
                relative.display()
            ));
        }
    }
    Ok(())
}

struct TempDirectory(PathBuf);
impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn temporary() -> TaskResult<TempDirectory> {
    for attempt in 0..100 {
        let path = std::env::temp_dir().join(format!(
            "port-meridian-codegen-{}-{attempt}",
            std::process::id()
        ));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(TempDirectory(path)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.to_string()),
        }
    }
    Err("Unable to create codegen comparison directory".into())
}

pub fn generate() -> TaskResult<()> {
    let files = artifacts()?;
    write_files(&workspace(), &files)?;
    println!("Generated {} protocol/fixture files.", files.len());
    Ok(())
}

pub fn check() -> TaskResult<()> {
    let files = artifacts()?;
    let temp = temporary()?;
    write_files(&temp.0, &files)?;
    compare(&workspace(), &temp.0, &files)?;
    println!("Generated protocol and fixtures match.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drift_detection_is_read_only_and_catches_extra_files() {
        let root = temporary().expect("root");
        let regenerated = temporary().expect("regenerated");
        let files = artifacts().expect("artifacts");
        write_files(&root.0, &files).expect("write");
        write_files(&regenerated.0, &files).expect("write");
        assert!(compare(&root.0, &regenerated.0, &files).is_ok());
        let first = files.keys().next().expect("file");
        let path = root.0.join(first);
        fs::write(&path, b"intentional drift").expect("drift");
        assert!(compare(&root.0, &regenerated.0, &files).is_err());
        assert_eq!(fs::read(&path).expect("read"), b"intentional drift");
        write_files(&root.0, &files).expect("restore");
        fs::write(root.0.join(TYPES_ROOT).join("extra.ts"), b"").expect("extra");
        assert!(compare(&root.0, &regenerated.0, &files).is_err());
    }
}
