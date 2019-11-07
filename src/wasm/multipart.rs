//! multipart/form-data
use std::borrow::Cow;
use std::fmt;

use bytes::Bytes;

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
    type ImplStream = crate::wasm::body::ImplStream;

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
        Body::stream(stream)
    }

    fn into_stream(self) -> Self::ImplStream {
        Body::into_stream(self)
    }
}
