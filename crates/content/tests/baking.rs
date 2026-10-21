use content::{Authoring, ColliderRole, CollisionPackage, VisualChunk, bake, canonical, validate};
fn source() -> Authoring {
    serde_json::from_str(include_str!("../../../content/source/slice.json"))
        .expect("authored slice")
}
fn collision(package: &bake::BakedPackage) -> CollisionPackage {
    serde_json::from_slice(&package.files["collision.json"]).unwrap()
}
fn rehash(package: &mut bake::BakedPackage) {
    package.manifest.content_hash = canonical::manifest_hash(&package.manifest).unwrap();
}
#[test]
fn repeated_bakes_are_byte_identical_and_cover_the_slice() {
    let a = bake::bake(&source()).unwrap();
    let b = bake::bake(&source()).unwrap();
    assert_eq!(a.files, b.files);
    assert_eq!(a.manifest, b.manifest);
    assert_eq!(a.manifest.chunks.len(), 4);
    assert_eq!(a.manifest.chunks[0].cx, 1);
    assert_eq!(a.manifest.chunks[0].cz, 3);
    assert_eq!(a.manifest.spawns[0].position, [-180.0, 0.0, 180.0]);
    assert!(a.files["collision.json"].len() < 2 * 1024 * 1024);
    assert!(
        collision(&a)
            .colliders
            .iter()
            .filter(|c| c.role == ColliderRole::Building)
            .count()
            >= 12
    );
    assert_eq!(
        a.manifest.road_graph.accesses.len(),
        source().accesses.len()
    );
    assert_eq!(a.manifest.vehicle_visuals[0].id, "compact_greybox");
}
#[test]
fn canonical_numbers_keys_and_invalid_domains() {
    assert_eq!(
        String::from_utf8(
            canonical::bytes(&serde_json::json!({"z":-0.0,"a":[1.0,0.000001,0.35,1000000.0]}))
                .unwrap()
        )
        .unwrap(),
        "{\"a\":[1,0.000001,0.35,1000000],\"z\":0}"
    );
    for n in [f64::NAN, f64::INFINITY, 1_000_001.0, 0.0000001] {
        assert!(canonical::number(n).is_err());
    }
    assert!(canonical::bytes(&serde_json::json!({"é":1})).is_err());
}
#[test]
fn content_and_simulation_hashes_have_distinct_coverage() {
    let base = bake::bake(&source()).unwrap();
    let mut changed = source();
    changed.sim_config.character.gravity = 19.0;
    let sim = bake::bake(&changed).unwrap();
    assert_ne!(base.manifest.content_hash, sim.manifest.content_hash);
    assert_ne!(base.manifest.sim_config_hash, sim.manifest.sim_config_hash);
    assert_eq!(
        base.manifest.collision_sha256,
        sim.manifest.collision_sha256
    );
    let mut changed = source();
    changed.buildings.seed += 1;
    let art = bake::bake(&changed).unwrap();
    assert_ne!(base.manifest.content_hash, art.manifest.content_hash);
    assert_eq!(base.manifest.sim_config_hash, art.manifest.sim_config_hash);
}
#[test]
fn overlapping_spawn_is_rejected() {
    let mut p = bake::bake(&source()).unwrap();
    let c = collision(&p);
    let wall = c
        .colliders
        .iter()
        .find(|c| c.role == ColliderRole::Building)
        .unwrap();
    p.manifest.spawns[0].position = [wall.center[0], 0.0, wall.center[2]];
    rehash(&mut p);
    assert!(
        validate::package(&p.manifest, &c, &p.files)
            .unwrap_err()
            .contains("spawn spawn_quay overlaps")
    );
}
#[test]
fn broken_graph_connectivity_and_dangling_edges_fail() {
    let mut p = bake::bake(&source()).unwrap();
    let c = collision(&p);
    p.manifest.road_graph.edges.clear();
    rehash(&mut p);
    assert!(
        validate::package(&p.manifest, &c, &p.files)
            .unwrap_err()
            .contains("not strongly connected")
    );
    let mut p = bake::bake(&source()).unwrap();
    p.manifest.road_graph.edges[0].to = 999999;
    rehash(&mut p);
    assert!(
        validate::package(&p.manifest, &c, &p.files)
            .unwrap_err()
            .contains("missing graph edge")
    );
}
#[test]
fn references_corridors_driveways_and_nonfinite_source_fail() {
    let mut a = source();
    a.parking[0].template_id = "missing".into();
    assert!(bake::bake(&a).is_err());
    let mut a = source();
    a.zones[0].allowed_templates.push("missing".into());
    assert!(bake::bake(&a).is_err());
    let mut a = source();
    a.accesses[0].driveway_id = "missing".into();
    assert!(bake::bake(&a).is_err());
    let mut a = source();
    a.buildings.setback = 8.0;
    assert!(bake::bake(&a).is_err());
    let mut a = source();
    a.landmarks[0].position = [-128.0, 0.0, 64.0];
    assert!(bake::bake(&a).unwrap_err().contains("road corridor"));
    let mut a = source();
    a.sim_config.character.gravity = f64::NAN;
    assert!(bake::bake(&a).is_err());
}
#[test]
fn asset_tampering_and_visual_face_drift_fail() {
    let mut p = bake::bake(&source()).unwrap();
    let c = collision(&p);
    p.files.get_mut("collision.json").unwrap().push(b' ');
    assert!(
        validate::package(&p.manifest, &c, &p.files)
            .unwrap_err()
            .contains("asset hash mismatch")
    );
    let mut p = bake::bake(&source()).unwrap();
    let url = p.manifest.chunks[0].visual_url.clone();
    let mut visual: VisualChunk = serde_json::from_slice(&p.files[&url]).unwrap();
    let solid = visual
        .primitives
        .iter_mut()
        .find(|p| p.collider_id.is_some())
        .unwrap();
    solid.half_extents[0] -= 0.1;
    let bytes = canonical::bytes(&visual).unwrap();
    p.manifest.chunks[0].sha256 = canonical::sha256(&bytes);
    p.files.insert(url, bytes);
    rehash(&mut p);
    assert!(
        validate::package(&p.manifest, &c, &p.files)
            .unwrap_err()
            .contains("face mismatch")
    );
}
#[test]
fn duplicate_colliders_and_unsafe_asset_urls_fail() {
    let mut p = bake::bake(&source()).unwrap();
    let mut c = collision(&p);
    c.colliders[1].id = c.colliders[0].id;
    assert!(
        validate::package(&p.manifest, &c, &p.files)
            .unwrap_err()
            .contains("collider")
    );
    p.manifest.chunks[0].visual_url = "../outside.json".into();
    rehash(&mut p);
    assert!(
        validate::package(&p.manifest, &collision(&p), &p.files)
            .unwrap_err()
            .contains("unsafe asset path")
    );
}
