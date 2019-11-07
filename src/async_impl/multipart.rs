//! multipart/form-data
use std::borrow::Cow;
use std::fmt;

use bytes::Bytes;

use futures_util::TryStreamExt;

use super::Body;
use crate::multipart_detail;

/// An async multipart/form-data request.
pub struct Form(multipart_detail::Form<Body>);

/// A field in a multipart form.
pub struct Part(multipart_detail::Part<Body>);

// ===== impl Form =====

impl Form {
    /// Creates a new async Form without any content.
    pub fn new() -> Form {
        Form(multipart_detail::Form::new())
    }

    /// Get the boundary that this form will use.
    #[inline]
    pub fn boundary(&self) -> &str {
        self.0.boundary()
    }

    /// Add a data field with supplied name and value.
    ///
    /// # Examples
    ///
    /// ```
    /// let form = reqwest::multipart::Form::new()
    ///     .text("username", "seanmonstar")
    ///     .text("password", "secret");
    /// ```
    pub fn text<T, U>(self, name: T, value: U) -> Form
    where
        T: Into<Cow<'static, str>>,
        U: Into<Cow<'static, str>>,
    {
        Form(self.0.text(name, value))
    }

    /// Adds a customized Part.
    pub fn part<T>(self, name: T, part: Part) -> Form
    where
        T: Into<Cow<'static, str>>,
    {
        Form(self.0.part(name, part.0))
    }

    /// Configure this `Form` to percent-encode using the `path-segment` rules.
    pub fn percent_encode_path_segment(self) -> Form {
        Form(self.0.percent_encode_path_segment())
    }

    /// Configure this `Form` to percent-encode using the `attr-char` rules.
    pub fn percent_encode_attr_chars(self) -> Form {
        Form(self.0.percent_encode_attr_chars())
    }

    /// Configure this `Form` to skip percent-encoding
    pub fn percent_encode_noop(self) -> Form {
        Form(self.0.percent_encode_noop())
    }

    pub(crate) fn stream(self) -> Body {
        self.0.stream()
    }

    pub(crate) fn compute_length(&mut self) -> Option<u64> {
        self.0.compute_length()
    }
}

impl fmt::Debug for Form {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

// ===== impl Part =====

impl Part {
    /// Makes a text parameter.
    pub fn text<T>(value: T) -> Part
    where
        T: Into<Cow<'static, str>>,
    {
        Part(multipart_detail::Part::text(value))
    }

    /// Makes a new parameter from arbitrary bytes.
    pub fn bytes<T>(value: T) -> Part
    where
        T: Into<Cow<'static, [u8]>>,
    {
        Part(multipart_detail::Part::bytes(value))
    }

    /// Makes a new parameter from an arbitrary stream.
    pub fn stream<T: Into<Body>>(value: T) -> Part {
        Part(multipart_detail::Part::stream(value))
    }

    /// Tries to set the mime of this part.
    pub fn mime_str(self, mime: &str) -> crate::Result<Part> {
        self.0.mime_str(mime).map(Part)
    }

    /// Sets the filename, builder style.
    pub fn file_name<T>(self, filename: T) -> Part
    where
        T: Into<Cow<'static, str>>,
    {
        Part(self.0.file_name(filename))
    }
}

impl fmt::Debug for Part {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl multipart_detail::MultipartBody for Body {
    type ImplStream = crate::async_impl::body::ImplStream;

    fn empty() -> Self {
        Body::empty()
    }

    fn content_length(&self) -> Option<u64> {
        Body::content_length(&self)
    }

    fn stream<S>(stream: S) -> Self
    where
        S: futures_core::stream::TryStream + Send + Sync + 'static,
        S::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
        Bytes: From<S::Ok>,
    {
        Body::stream(stream.map_ok(Bytes::from))
    }

    fn into_stream(self) -> Self::ImplStream {
        Body::into_stream(self)
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::multipart_detail::{PercentEncoding};
    use futures_util::{StreamExt, TryStreamExt};
    use futures_util::{future, stream};
    use tokio;

    #[test]
    fn form_empty() {
        let form = Form::new();

        let mut rt = tokio::runtime::current_thread::Runtime::new().expect("new rt");
        let body = form.stream().into_stream();
        let s = body.map(|try_c| try_c.map(Bytes::from)).try_concat();

        let out = rt.block_on(s);
        assert_eq!(out.unwrap(), Vec::new());
    }

    #[test]
    fn stream_to_end() {
        let mut form = Form::new()
            .part(
                "reader1",
                Part::stream(Body::stream(stream::once(future::ready::<
                    Result<String, crate::Error>,
                >(Ok(
                    "part1".to_owned(),
                ))))),
            )
            .part("key1", Part::text("value1"))
            .part("key2", Part(Part::text("value2").0.mime(mime::IMAGE_BMP)))
            .part(
                "reader2",
                Part::stream(Body::stream(stream::once(future::ready::<
                    Result<String, crate::Error>,
                >(Ok(
                    "part2".to_owned(),
                ))))),
            )
            .part("key3", Part::text("value3").file_name("filename"));
        form.0.inner.boundary = "boundary".to_string();
        let expected =
            "--boundary\r\n\
             Content-Disposition: form-data; name=\"reader1\"\r\n\r\n\
             part1\r\n\
             --boundary\r\n\
             Content-Disposition: form-data; name=\"key1\"\r\n\r\n\
             value1\r\n\
             --boundary\r\n\
             Content-Disposition: form-data; name=\"key2\"\r\n\
             Content-Type: image/bmp\r\n\r\n\
             value2\r\n\
             --boundary\r\n\
             Content-Disposition: form-data; name=\"reader2\"\r\n\r\n\
             part2\r\n\
             --boundary\r\n\
             Content-Disposition: form-data; name=\"key3\"; filename=\"filename\"\r\n\r\n\
             value3\r\n--boundary--\r\n";
        let mut rt = tokio::runtime::current_thread::Runtime::new().expect("new rt");
        let body = form.stream().into_stream();
        let s = body.map(|try_c| try_c.map(Bytes::from)).try_concat();

        let out = rt.block_on(s).unwrap();
        // These prints are for debug purposes in case the test fails
        println!(
            "START REAL\n{}\nEND REAL",
            std::str::from_utf8(&out).unwrap()
        );
        println!("START EXPECTED\n{}\nEND EXPECTED", expected);
        assert_eq!(std::str::from_utf8(&out).unwrap(), expected);
    }

    #[test]
    fn stream_to_end_with_header() {
        let mut part = Part(Part::text("value2").0.mime(mime::IMAGE_BMP));
        part.0.meta.headers.insert("Hdr3", "/a/b/c".parse().unwrap());
        let mut form = Form::new().part("key2", part);
        form.0.inner.boundary = "boundary".to_string();
        let expected = "--boundary\r\n\
                        Content-Disposition: form-data; name=\"key2\"\r\n\
                        Content-Type: image/bmp\r\n\
                        hdr3: /a/b/c\r\n\
                        \r\n\
                        value2\r\n\
                        --boundary--\r\n";
        let mut rt = tokio::runtime::current_thread::Runtime::new().expect("new rt");
        let body = form.stream().into_stream();
        let s = body.map(|try_c| try_c.map(Bytes::from)).try_concat();

        let out = rt.block_on(s).unwrap();
        // These prints are for debug purposes in case the test fails
        println!(
            "START REAL\n{}\nEND REAL",
            std::str::from_utf8(&out).unwrap()
        );
        println!("START EXPECTED\n{}\nEND EXPECTED", expected);
        assert_eq!(std::str::from_utf8(&out).unwrap(), expected);
    }

    #[test]
    fn header_percent_encoding() {
        let name = "start%'\"\r\nßend";
        let field = Part::text("");

        assert_eq!(
            PercentEncoding::PathSegment.encode_headers(name, &field.0.meta),
            &b"Content-Disposition: form-data; name*=utf-8''start%25'%22%0D%0A%C3%9Fend"[..]
        );

        assert_eq!(
            PercentEncoding::AttrChar.encode_headers(name, &field.0.meta),
            &b"Content-Disposition: form-data; name*=utf-8''start%25%27%22%0D%0A%C3%9Fend"[..]
        );
    }
}
