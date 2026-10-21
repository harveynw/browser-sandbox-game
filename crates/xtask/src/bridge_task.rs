use sim_wasm::{bridge::*, contract};
use std::{collections::BTreeMap, fs, path::Path};
pub fn run(check: bool) -> Result<(), String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let base = root.join("apps/client/src/simulation/generated");
    let mut files = BTreeMap::new();
    files.insert("types.ts", contract::typescript().into_bytes());
    macro_rules! schema {
        ($name:literal,$ty:ty) => {
            files.insert(
                $name,
                serde_json::to_vec_pretty(&schemars::schema_for!($ty))
                    .map_err(|e| e.to_string())?,
            );
        };
    }
    schema!("frame.schema.json", PredictedFrame);
    schema!("step.schema.json", StepRequest);
    schema!("proxies.schema.json", ProxySet);
    schema!("ray-query.schema.json", StaticRayQuery);
    schema!("sphere-query.schema.json", StaticSphereQuery);
    schema!("ray-result.schema.json", StaticRayResult);
    if !check {
        fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    }
    for (name, bytes) in &files {
        let path = base.join(name);
        if check {
            if fs::read(&path)
                .map_err(|e| format!("{}: {e}; run npm run bridge:generate", path.display()))?
                != *bytes
            {
                return Err(format!("bridge contract drift: {name}"));
            }
        } else {
            fs::write(path, bytes).map_err(|e| e.to_string())?;
        }
    }
    if check && fs::read_dir(&base).map_err(|e| e.to_string())?.count() != files.len() {
        return Err("unexpected bridge contract file".into());
    }
    println!(
        "Bridge contracts {}: {} files",
        if check { "verified" } else { "generated" },
        files.len()
    );
    Ok(())
}
