pub use self::body::Body;

if_hyper! {
	pub use self::client::{Client, ClientBuilder};
	pub(crate) use self::decoder::Decoder;
	pub use self::request::{Request, RequestBuilder};
	pub use self::response::{Response, ResponseBuilderExt};
}

pub mod body;

if_hyper! {
	pub mod client;
	pub mod decoder;
}

pub mod multipart;

if_hyper! {
	pub(crate) mod request;
	mod response;
}
