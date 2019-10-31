use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub async fn yoyo() -> String {
	let client = reqwest::Client::new();
	let resp = client.get("https://httpbin.org/json").send().await.unwrap();
	format!("{}", resp.url())
}
