//! Local MessagePack bridge to the shared motor. No networking.
pub mod bridge;
pub mod contract;
use wasm_bindgen::prelude::*;

/// Diagnostic export proving that the shared core builds for WebAssembly.
#[wasm_bindgen]
pub fn simulation_tick_rate() -> u32 {
    sim_core::TICK_RATE
}

#[wasm_bindgen]
pub struct WasmPredictionWorld {
    inner: bridge::PredictionWorld,
}
fn js_error(error: String) -> JsValue {
    JsValue::from_str(&error)
}
#[wasm_bindgen]
impl WasmPredictionWorld {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            inner: bridge::PredictionWorld::default(),
        }
    }
    pub fn load_collision(&mut self, collision: &[u8], config: &[u8]) -> Result<(), JsValue> {
        self.inner
            .load_collision(collision, config)
            .map_err(js_error)
    }
    pub fn set_vehicle_proxies(&mut self, bytes: &[u8]) -> Result<(), JsValue> {
        self.inner.set_vehicle_proxies(bytes).map_err(js_error)
    }
    pub fn reset(&mut self, bytes: &[u8]) -> Result<(), JsValue> {
        self.inner.reset(bytes).map_err(js_error)
    }
    pub fn step(&mut self, bytes: &[u8]) -> Result<Vec<u8>, JsValue> {
        self.inner.step(bytes).map_err(js_error)
    }
    pub fn current_state(&self) -> Result<Vec<u8>, JsValue> {
        self.inner.current_state().map_err(js_error)
    }
    pub fn spherecast_static(&self, bytes: &[u8]) -> Result<Vec<u8>, JsValue> {
        self.inner.spherecast_static(bytes).map_err(js_error)
    }
    pub fn raycast_static(&self, bytes: &[u8]) -> Result<Vec<u8>, JsValue> {
        self.inner.raycast_static(bytes).map_err(js_error)
    }
}
impl Default for WasmPredictionWorld {
    fn default() -> Self {
        Self::new()
    }
}
