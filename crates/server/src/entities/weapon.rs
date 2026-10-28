//! Equipped weapon ammunition and lifecycle timers.
#[derive(Clone)]
pub struct Weapon {
    pub magazine: u8,
    pub next_fire: u32,
    pub reload_end: Option<u32>,
    pub respawn: Option<u32>,
    pub protected_until: Option<u32>,
}
impl Default for Weapon {
    fn default() -> Self {
        Self {
            magazine: 12,
            next_fire: 0,
            reload_end: None,
            respawn: None,
            protected_until: None,
        }
    }
}
