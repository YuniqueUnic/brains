use js_sys::Promise;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(module = "/src/bindings.js")]
extern "C" {
    pub fn markdown(env: &JsValue, name: &str, bytes: &[u8], media: &str) -> Promise;
    #[wasm_bindgen(js_name = fetchPublic)]
    pub fn fetch_public(url: &str, max_bytes: usize) -> Promise;
    #[wasm_bindgen(js_name = searchQuery)]
    pub fn search_query(
        env: &JsValue,
        instance: &str,
        query: &str,
        limit: usize,
        threshold: f64,
    ) -> Promise;
    #[wasm_bindgen(js_name = findItem)]
    pub fn find_item(env: &JsValue, instance: &str, key: &str) -> Promise;
    #[wasm_bindgen(js_name = uploadItem)]
    pub fn upload_item(env: &JsValue, instance: &str, key: &str, body: &str) -> Promise;
    #[wasm_bindgen(js_name = deleteItem)]
    pub fn delete_item(env: &JsValue, instance: &str, id: &str) -> Promise;
}
