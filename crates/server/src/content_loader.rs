use content::{CollisionPackage, PackageIndex, SimConfig, WorldManifest, canonical};
use std::{collections::BTreeMap, fs::File, io::Read, path::Path};
pub struct WorldContent {
    pub manifest: WorldManifest,
    pub config: SimConfig,
    pub collision: CollisionPackage,
}
fn read(path: &Path, max: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > max {
        return Err("content file budget".into());
    }
    Ok(bytes)
}
fn hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn asset(value: &str) -> bool {
    !value.contains('/')
        && !value.contains('\\')
        && !value.contains("..")
        && value.ends_with(".json")
}
impl WorldContent {
    /// Runtime reads the published package, never rebakes or trusts a source-only hash.
    pub fn load(root: &Path) -> Result<Self, String> {
        Self::load_index(root, "slice.json")
    }
    pub fn load_index(root: &Path, index_name: &str) -> Result<Self, String> {
        let index: PackageIndex = serde_json::from_slice(&read(&root.join(index_name), 4096)?)
            .map_err(|e| e.to_string())?;
        if index.schema_version != 1
            || !hash(&index.content_hash)
            || !hash(&index.manifest_sha256)
            || index.manifest_url != format!("packages/{}/manifest.json", index.content_hash)
        {
            return Err("invalid package index/path".into());
        }
        let base = root.join("packages").join(&index.content_hash);
        let manifest_bytes = read(&base.join("manifest.json"), 2 * 1024 * 1024)?;
        if canonical::sha256(&manifest_bytes) != index.manifest_sha256 {
            return Err("manifest byte hash mismatch".into());
        }
        let manifest: WorldManifest =
            serde_json::from_slice(&manifest_bytes).map_err(|e| e.to_string())?;
        if manifest.weapons != protocol::weapon_catalogue()
            || manifest.schema_version != 1
            || manifest.content_hash != index.content_hash
            || canonical::manifest_hash(&manifest)? != index.content_hash
            || manifest.chunks.len() > 64
            || manifest.vehicle_visuals.len() > 64
            || manifest.spawns.is_empty()
            || manifest.spawns.len() > 64
        {
            return Err("manifest version/hash/count mismatch".into());
        }
        let mut files = BTreeMap::new();
        let refs = std::iter::once((&manifest.collision_url, &manifest.collision_sha256))
            .chain(std::iter::once((
                &manifest.sim_config_url,
                &manifest.sim_config_hash,
            )))
            .chain(std::iter::once((
                &manifest.debug_url,
                &manifest.debug_sha256,
            )))
            .chain([
                (&manifest.backdrop.url, &manifest.backdrop.sha256),
                (&manifest.minimap.url, &manifest.minimap.sha256),
            ])
            .chain(manifest.chunks.iter().map(|c| (&c.visual_url, &c.sha256)))
            .chain(manifest.vehicle_visuals.iter().map(|v| (&v.url, &v.sha256)));
        let mut total = manifest_bytes.len();
        for (url, expected) in refs {
            if !asset(url) || !hash(expected) || files.contains_key(url) {
                return Err("unsafe/duplicate asset reference".into());
            }
            let bytes = read(&base.join(url), 2 * 1024 * 1024)?;
            total += bytes.len();
            if total > 16 * 1024 * 1024 || canonical::sha256(&bytes) != *expected {
                return Err("asset byte hash/budget mismatch".into());
            }
            files.insert(url.clone(), bytes);
        }
        let config: SimConfig =
            serde_json::from_slice(files.get(&manifest.sim_config_url).ok_or("config")?)
                .map_err(|e| e.to_string())?;
        let collision: CollisionPackage =
            serde_json::from_slice(files.get(&manifest.collision_url).ok_or("collision")?)
                .map_err(|e| e.to_string())?;
        content::validate::package(&manifest, &collision, &files)?;
        let runtime = sim_core::SimConfig::from_content(&config)?;
        sim_core::CollisionWorld::from_content(&collision, &runtime)?;
        Ok(Self {
            manifest,
            config,
            collision,
        })
    }
}
