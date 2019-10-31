use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub async fn yoyo() -> Result<String, reqwest::Error> {
	let client = reqwest::Client::new();
	let resp = client
		.get("https://httpbin.org/headers")
		.header(reqwest::header::REFERER, 1234)
		.send()
		.await?;
	Ok(resp.text().await?)
}
