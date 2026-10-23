use sim_core::collision::{ray_character, ray_vehicle, vehicle_hits_character};
#[test]
fn hit_rays_respect_capsule_box_range_and_rotation() {
    assert!(
        ray_character([0.0, 1.4, 10.0], [0.0, 0.0, -1.0], [0.0, 0.02, 0.0], 80.0).unwrap() < 10.0
    );
    assert!(ray_character([1.0, 1.4, 10.0], [0.0, 0.0, -1.0], [0.0, 0.02, 0.0], 80.0).is_none());
    assert!(
        ray_vehicle(
            [0.0, 1.0, 10.0],
            [0.0, 0.0, -1.0],
            [0.0, 0.0, 0.0],
            0.0,
            [2.0, 1.4, 4.0],
            7.0
        )
        .is_none()
    );
    assert!(
        ray_vehicle(
            [0.0, 1.0, 10.0],
            [0.0, 0.0, -1.0],
            [0.0, 0.0, 0.0],
            std::f32::consts::FRAC_PI_2,
            [2.0, 1.4, 4.0],
            80.0
        )
        .unwrap()
            > 8.9
    );
}
#[test]
fn swept_vehicle_detects_a_capsule_between_endpoints_and_rejects_near_misses() {
    assert!(vehicle_hits_character(
        [0.0, 0.0, 5.0],
        [0.0, 0.0, -5.0],
        [0.0, 0.0],
        [2.0, 1.4, 4.0],
        [0.0, 0.02, 0.0],
        [0.0, 0.02, 0.0]
    ));
    assert!(!vehicle_hits_character(
        [0.0, 0.0, 5.0],
        [0.0, 0.0, -5.0],
        [0.0, 0.0],
        [2.0, 1.4, 4.0],
        [3.0, 0.02, 0.0],
        [3.0, 0.02, 0.0]
    ));
    assert!(vehicle_hits_character(
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 0.0],
        [0.0, 0.0],
        [2.0, 1.4, 4.0],
        [-4.0, 0.02, 0.0],
        [4.0, 0.02, 0.0]
    ));
}
