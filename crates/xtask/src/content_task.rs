use content::{Authoring, PackageIndex, canonical, contract, types::*};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
type Files = BTreeMap<PathBuf, Vec<u8>>;
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn generated() -> Result<Files, String> {
    let base = "packages/content/src/generated";
    let mut files = Files::new();
    files.insert(
        format!("{base}/types.ts").into(),
        contract::typescript().into_bytes(),
    );
    macro_rules! schema {
        ($name:literal,$ty:ty) => {
            files.insert(
                format!("{base}/{}.schema.json", $name).into(),
                serde_json::to_vec_pretty(&contract::schema::<$ty>()).map_err(|e| e.to_string())?,
            );
        };
    }
    schema!("authoring", Authoring);
    schema!("manifest", WorldManifest);
    schema!("collision", CollisionPackage);
    schema!("chunk", VisualChunk);
    schema!("debug", DebugOverlay);
    schema!("sim-config", SimConfig);
    schema!("index", PackageIndex);
    let example = serde_json::json!({"z":-0.0,"a":[1.0,0.000001,0.35,1000000.0,"ASCII content"],"nested":{"b":2,"a":true}});
    let bytes = canonical::bytes(&example)?;
    files.insert(format!("{base}/canonical-fixture.json").into(),serde_json::to_vec_pretty(&serde_json::json!({"value":example,"canonical":String::from_utf8(bytes.clone()).map_err(|e|e.to_string())?,"sha256":canonical::sha256(&bytes)})).map_err(|e|e.to_string())?);
    Ok(files)
}
fn baked_source(name: &str) -> Result<(Files, WorldManifest), String> {
    let source: Authoring = serde_json::from_slice(
        &fs::read(root().join(format!("content/source/{name}.json"))).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let package = content::bake::bake(&source)?;
    let hash = &package.manifest.content_hash;
    let mut files = Files::new();
    for (name, bytes) in &package.files {
        files.insert(
            format!("apps/client/public/content/packages/{hash}/{name}").into(),
            bytes.clone(),
        );
    }
    let index = PackageIndex {
        schema_version: 1,
        content_hash: hash.clone(),
        manifest_url: format!("packages/{hash}/manifest.json"),
        manifest_sha256: canonical::sha256(
            package
                .files
                .get("manifest.json")
                .ok_or("manifest missing")?,
        ),
    };
    files.insert(
        format!("apps/client/public/content/{name}.json").into(),
        canonical::bytes(&index)?,
    );
    Ok((files, package.manifest))
}
fn baked() -> Result<(Files, WorldManifest), String> {
    let (mut slice, _) = baked_source("slice")?;
    let (city, manifest) = baked_source("city")?;
    slice.extend(city);
    Ok((slice, manifest))
}
fn write(files: &Files) -> Result<(), String> {
    for (relative, bytes) in files {
        let path = root().join(relative);
        fs::create_dir_all(path.parent().ok_or("no parent")?).map_err(|e| e.to_string())?;
        // A hash-addressed package is immutable; never silently overwrite corruption.
        if relative.starts_with("apps/client/public/content/packages") && path.exists() {
            if fs::read(&path).map_err(|e| e.to_string())? != *bytes {
                return Err(format!("immutable asset differs: {}", relative.display()));
            }
            continue;
        }
        let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
        fs::write(&temporary, bytes).map_err(|e| e.to_string())?;
        fs::rename(temporary, path).map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn compare(files: &Files) -> Result<(), String> {
    compare_at(&root(), files)
}
fn compare_at(base: &Path, files: &Files) -> Result<(), String> {
    for (path, bytes) in files {
        let actual = fs::read(base.join(path))
            .map_err(|e| format!("{}: {e}; run npm run content:bake", path.display()))?;
        if actual != *bytes {
            return Err(format!(
                "content drift: {}; run npm run content:bake",
                path.display()
            ));
        }
    }
    // Extra files inside current package/generated contracts are also drift.
    let mut directories = std::collections::BTreeSet::new();
    for path in files.keys() {
        directories.insert(path.parent().ok_or("missing parent")?.to_path_buf());
    }
    for dir in directories {
        // content root may retain older immutable packages; only inspect this package.
        if dir == Path::new("apps/client/public/content") {
            continue;
        }
        for entry in fs::read_dir(base.join(&dir)).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            if !files.contains_key(&dir.join(entry.file_name())) {
                return Err(format!("extra generated asset: {}", entry.path().display()));
            }
        }
    }
    Ok(())
}
pub fn generate() -> Result<(), String> {
    let files = generated()?;
    write(&files)?;
    println!("Generated {} content contract/fixture files", files.len());
    Ok(())
}
pub fn bake() -> Result<(), String> {
    generate()?;
    let (files, m) = baked()?;
    // Publish index only after every immutable asset has been written.
    let indexes = [
        PathBuf::from("apps/client/public/content/slice.json"),
        PathBuf::from("apps/client/public/content/city.json"),
    ];
    let assets = files
        .iter()
        .filter(|(k, _)| !indexes.contains(k))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    write(&assets)?;
    for index in indexes {
        write(&BTreeMap::from([(index.clone(), files[&index].clone())]))?;
    }
    println!(
        "Baked {} assets: {} ({} road nodes, {} pedestrian nodes)",
        files.len(),
        m.content_hash,
        m.road_graph.nodes.len(),
        m.pedestrian_graph.nodes.len()
    );
    Ok(())
}
pub fn check() -> Result<(), String> {
    compare(&generated()?)?;
    let (files, m) = baked()?;
    compare(&files)?;
    println!(
        "Content contracts and baked package match {}",
        m.content_hash
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn content_drift_check_is_read_only_and_rejects_extra_files() {
        let path = std::env::temp_dir().join(format!(
            "port-meridian-content-check-{}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(path.clone());
        let relative = PathBuf::from("packages/content/src/generated/types.ts");
        fs::create_dir_all(path.join(relative.parent().unwrap())).unwrap();
        fs::write(path.join(&relative), b"reference").unwrap();
        let files = BTreeMap::from([(relative.clone(), b"reference".to_vec())]);
        assert!(compare_at(&path, &files).is_ok());
        fs::write(path.join(&relative), b"intentional drift").unwrap();
        assert!(compare_at(&path, &files).is_err());
        assert_eq!(
            fs::read(path.join(&relative)).unwrap(),
            b"intentional drift"
        );
        fs::write(path.join(&relative), b"reference").unwrap();
        fs::write(
            path.join(relative.parent().unwrap()).join("extra.json"),
            b"{}",
        )
        .unwrap();
        assert!(compare_at(&path, &files).is_err());
    }
}
