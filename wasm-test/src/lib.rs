use wasm_bindgen::prelude::*;
use reqwest::multipart::Form;

#[wasm_bindgen]
pub async fn yoyo() -> Result<String, reqwest::Error> {
	let client = reqwest::Client::new();
	let resp = client
		.post("https://httpbin.org/post")
		.header(reqwest::header::REFERER, "1234")
		.query(&[("username", "admin"), ("password", "hunter2")])
		.multipart(Form::new()
			.text("Yoyo", "koyo")
			.text("Kala", "głaskanie"))
		.send()
		.await?;
	Ok(resp.text().await?)
}
