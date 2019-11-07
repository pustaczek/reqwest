use std::borrow::Cow;
use std::fmt;
use std::pin::Pin;

use bytes::Bytes;
use http::HeaderMap;
use mime_guess::Mime;
use percent_encoding::{self, AsciiSet, NON_ALPHANUMERIC};

use futures_core::Stream;
use futures_util::{future, stream, StreamExt};

pub(crate) trait MultipartBody: fmt::Debug + From<&'static str> + From<String> + From<&'static [u8]> + From<Vec<u8>> + 'static {
    type ImplStream: Stream<Item = Result<Bytes, crate::Error>> + Send + Sync;
    fn empty() -> Self;
    fn content_length(&self) -> Option<u64>;
    fn stream<S>(stream: S) -> Self
    where
        S: futures_core::stream::TryStream + Send + Sync + 'static,
        S::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
        Bytes: From<S::Ok>;
    fn into_stream(self) -> Self::ImplStream;    
}

/// A multipart/form-data request.
pub(crate) struct Form<B: MultipartBody> {
    inner: FormParts<Part<B>>,
}

/// A field in a multipart form.
pub(crate) struct Part<B: MultipartBody> {
    meta: PartMetadata,
    value: B,
}

pub(crate) struct FormParts<P> {
    pub(crate) boundary: String,
    pub(crate) computed_headers: Vec<Vec<u8>>,
    pub(crate) fields: Vec<(Cow<'static, str>, P)>,
    pub(crate) percent_encoding: PercentEncoding,
}

pub(crate) struct PartMetadata {
    mime: Option<Mime>,
    file_name: Option<Cow<'static, str>>,
    pub(crate) headers: HeaderMap,
}

pub(crate) trait PartProps {
    fn value_len(&self) -> Option<u64>;
    fn metadata(&self) -> &PartMetadata;
}

// ===== impl Form =====

impl<B: MultipartBody> Form<B> {
    /// Creates a new async Form without any content.
    pub fn new() -> Form<B> {
        Form {
            inner: FormParts::new(),
        }
    }

    /// Get the boundary that this form will use.
    #[inline]
    pub fn boundary(&self) -> &str {
        self.inner.boundary()
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
    pub fn text<T, U>(self, name: T, value: U) -> Form<B>
    where
        T: Into<Cow<'static, str>>,
        U: Into<Cow<'static, str>>,
    {
        self.part(name, Part::text(value))
    }

    /// Adds a customized Part.
    pub fn part<T>(self, name: T, part: Part<B>) -> Form<B>
    where
        T: Into<Cow<'static, str>>,
    {
        self.with_inner(move |inner| inner.part(name, part))
    }

    /// Configure this `Form` to percent-encode using the `path-segment` rules.
    pub fn percent_encode_path_segment(self) -> Form<B> {
        self.with_inner(|inner| inner.percent_encode_path_segment())
    }

    /// Configure this `Form` to percent-encode using the `attr-char` rules.
    pub fn percent_encode_attr_chars(self) -> Form<B> {
        self.with_inner(|inner| inner.percent_encode_attr_chars())
    }

    /// Configure this `Form` to skip percent-encoding
    pub fn percent_encode_noop(self) -> Form<B> {
        self.with_inner(|inner| inner.percent_encode_noop())
    }

    /// Consume this instance and transform into an instance of Body for use in a request.
    pub(crate) fn stream(mut self) -> B {
        if self.inner.fields.is_empty() {
            return B::empty();
        }

        // create initial part to init reduce chain
        let (name, part) = self.inner.fields.remove(0);
        let start = Box::pin(self.part_stream(name, part))
            as Pin<Box<dyn Stream<Item = crate::Result<Bytes>> + Send + Sync>>;

        let fields = self.inner.take_fields();
        // for each field, chain an additional stream
        let stream = fields.into_iter().fold(start, |memo, (name, part)| {
            let part_stream = self.part_stream(name, part);
            Box::pin(memo.chain(part_stream))
                as Pin<Box<dyn Stream<Item = crate::Result<Bytes>> + Send + Sync>>
        });
        // append special ending boundary
        let last = stream::once(future::ready(Ok(
            format!("--{}--\r\n", self.boundary()).into()
        )));
        B::stream(stream.chain(last))
    }

    /// Generate a hyper::Body stream for a single Part instance of a Form request.
    pub(crate) fn part_stream<T>(
        &mut self,
        name: T,
        part: Part<B>,
    ) -> impl Stream<Item = Result<Bytes, crate::Error>>
    where
        T: Into<Cow<'static, str>>,
    {
        // start with boundary
        let boundary = stream::once(future::ready(Ok(
            format!("--{}\r\n", self.boundary()).into()
        )));
        // append headers
        let header = stream::once(future::ready(Ok({
            let mut h = self
                .inner
                .percent_encoding
                .encode_headers(&name.into(), &part.meta);
            h.extend_from_slice(b"\r\n\r\n");
            h.into()
        })));
        // then append form data followed by terminating CRLF
        boundary
            .chain(header)
            .chain(part.value.into_stream())
            .chain(stream::once(future::ready(Ok("\r\n".into()))))
    }

    pub(crate) fn compute_length(&mut self) -> Option<u64> {
        self.inner.compute_length()
    }

    fn with_inner<F>(self, func: F) -> Self
    where
        F: FnOnce(FormParts<Part<B>>) -> FormParts<Part<B>>,
    {
        Form {
            inner: func(self.inner),
        }
    }
}

impl<B: MultipartBody> fmt::Debug for Form<B> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.inner.fmt_fields("Form", f)
    }
}

// ===== impl Part =====

impl<B: MultipartBody> Part<B> {
    /// Makes a text parameter.
    pub fn text<T>(value: T) -> Part<B>
    where
        T: Into<Cow<'static, str>>,
    {
        let body = match value.into() {
            Cow::Borrowed(slice) => B::from(slice),
            Cow::Owned(string) => B::from(string),
        };
        Part::new(body)
    }

    /// Makes a new parameter from arbitrary bytes.
    pub fn bytes<T>(value: T) -> Part<B>
    where
        T: Into<Cow<'static, [u8]>>,
    {
        let body = match value.into() {
            Cow::Borrowed(slice) => B::from(slice),
            Cow::Owned(vec) => B::from(vec),
        };
        Part::new(body)
    }

    /// Makes a new parameter from an arbitrary stream.
    pub fn stream<T: Into<B>>(value: T) -> Part<B> {
        Part::new(value.into())
    }

    fn new(value: B) -> Part<B> {
        Part {
            meta: PartMetadata::new(),
            value,
        }
    }

    /// Tries to set the mime of this part.
    pub fn mime_str(self, mime: &str) -> crate::Result<Part<B>> {
        Ok(self.mime(mime.parse().map_err(crate::error::builder)?))
    }

    // Re-export when mime 0.4 is available, with split MediaType/MediaRange.
    pub(crate) fn mime(self, mime: Mime) -> Part<B> {
        self.with_inner(move |inner| inner.mime(mime))
    }

    /// Sets the filename, builder style.
    pub fn file_name<T>(self, filename: T) -> Part<B>
    where
        T: Into<Cow<'static, str>>,
    {
        self.with_inner(move |inner| inner.file_name(filename))
    }

    fn with_inner<F>(self, func: F) -> Self
    where
        F: FnOnce(PartMetadata) -> PartMetadata,
    {
        Part {
            meta: func(self.meta),
            value: self.value,
        }
    }
}

impl<B: MultipartBody> fmt::Debug for Part<B> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut dbg = f.debug_struct("Part");
        dbg.field("value", &self.value);
        self.meta.fmt_fields(&mut dbg);
        dbg.finish()
    }
}

impl<B: MultipartBody> PartProps for Part<B> {
    fn value_len(&self) -> Option<u64> {
        self.value.content_length()
    }

    fn metadata(&self) -> &PartMetadata {
        &self.meta
    }
}

// ===== impl FormParts =====

impl<P: PartProps> FormParts<P> {
    pub(crate) fn new() -> Self {
        FormParts {
            boundary: gen_boundary(),
            computed_headers: Vec::new(),
            fields: Vec::new(),
            percent_encoding: PercentEncoding::PathSegment,
        }
    }

    pub(crate) fn boundary(&self) -> &str {
        &self.boundary
    }

    /// Adds a customized Part.
    pub(crate) fn part<T>(mut self, name: T, part: P) -> Self
    where
        T: Into<Cow<'static, str>>,
    {
        self.fields.push((name.into(), part));
        self
    }

    /// Configure this `Form` to percent-encode using the `path-segment` rules.
    pub(crate) fn percent_encode_path_segment(mut self) -> Self {
        self.percent_encoding = PercentEncoding::PathSegment;
        self
    }

    /// Configure this `Form` to percent-encode using the `attr-char` rules.
    pub(crate) fn percent_encode_attr_chars(mut self) -> Self {
        self.percent_encoding = PercentEncoding::AttrChar;
        self
    }

    /// Configure this `Form` to skip percent-encoding
    pub(crate) fn percent_encode_noop(mut self) -> Self {
        self.percent_encoding = PercentEncoding::NoOp;
        self
    }

    // If predictable, computes the length the request will have
    // The length should be preditable if only String and file fields have been added,
    // but not if a generic reader has been added;
    pub(crate) fn compute_length(&mut self) -> Option<u64> {
        let mut length = 0u64;
        for &(ref name, ref field) in self.fields.iter() {
            match field.value_len() {
                Some(value_length) => {
                    // We are constructing the header just to get its length. To not have to
                    // construct it again when the request is sent we cache these headers.
                    let header = self.percent_encoding.encode_headers(name, field.metadata());
                    let header_length = header.len();
                    self.computed_headers.push(header);
                    // The additions mimick the format string out of which the field is constructed
                    // in Reader. Not the cleanest solution because if that format string is
                    // ever changed then this formula needs to be changed too which is not an
                    // obvious dependency in the code.
                    length += 2
                        + self.boundary().len() as u64
                        + 2
                        + header_length as u64
                        + 4
                        + value_length
                        + 2
                }
                _ => return None,
            }
        }
        // If there is a at least one field there is a special boundary for the very last field.
        if !self.fields.is_empty() {
            length += 2 + self.boundary().len() as u64 + 4
        }
        Some(length)
    }

    /// Take the fields vector of this instance, replacing with an empty vector.
    fn take_fields(&mut self) -> Vec<(Cow<'static, str>, P)> {
        std::mem::replace(&mut self.fields, Vec::new())
    }
}

impl<P: fmt::Debug> FormParts<P> {
    pub(crate) fn fmt_fields(&self, ty_name: &'static str, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct(ty_name)
            .field("boundary", &self.boundary)
            .field("parts", &self.fields)
            .finish()
    }
}

// ===== impl PartMetadata =====

impl PartMetadata {
    pub(crate) fn new() -> Self {
        PartMetadata {
            mime: None,
            file_name: None,
            headers: HeaderMap::default(),
        }
    }

    pub(crate) fn mime(mut self, mime: Mime) -> Self {
        self.mime = Some(mime);
        self
    }

    pub(crate) fn file_name<T>(mut self, filename: T) -> Self
    where
        T: Into<Cow<'static, str>>,
    {
        self.file_name = Some(filename.into());
        self
    }
}

impl PartMetadata {
    pub(crate) fn fmt_fields<'f, 'fa, 'fb>(
        &self,
        debug_struct: &'f mut fmt::DebugStruct<'fa, 'fb>,
    ) -> &'f mut fmt::DebugStruct<'fa, 'fb> {
        debug_struct
            .field("mime", &self.mime)
            .field("file_name", &self.file_name)
            .field("headers", &self.headers)
    }
}

/// https://url.spec.whatwg.org/#fragment-percent-encode-set
const FRAGMENT_ENCODE_SET: &AsciiSet = &percent_encoding::CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'<')
    .add(b'>')
    .add(b'`');

/// https://url.spec.whatwg.org/#path-percent-encode-set
const PATH_ENCODE_SET: &AsciiSet = &FRAGMENT_ENCODE_SET.add(b'#').add(b'?').add(b'{').add(b'}');

const PATH_SEGMENT_ENCODE_SET: &AsciiSet = &PATH_ENCODE_SET.add(b'/').add(b'%');

/// https://tools.ietf.org/html/rfc8187#section-3.2.1
const ATTR_CHAR_ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'!')
    .remove(b'#')
    .remove(b'$')
    .remove(b'&')
    .remove(b'+')
    .remove(b'-')
    .remove(b'.')
    .remove(b'^')
    .remove(b'_')
    .remove(b'`')
    .remove(b'|')
    .remove(b'~');

pub(crate) enum PercentEncoding {
    PathSegment,
    AttrChar,
    NoOp,
}

impl PercentEncoding {
    pub(crate) fn encode_headers(&self, name: &str, field: &PartMetadata) -> Vec<u8> {
        let s = format!(
            "Content-Disposition: form-data; {}{}{}",
            self.format_parameter("name", name),
            match field.file_name {
                Some(ref file_name) => format!("; {}", self.format_filename(file_name)),
                None => String::new(),
            },
            match field.mime {
                Some(ref mime) => format!("\r\nContent-Type: {}", mime),
                None => "".to_string(),
            },
        );
        field
            .headers
            .iter()
            .fold(s.into_bytes(), |mut header, (k, v)| {
                header.extend_from_slice(b"\r\n");
                header.extend_from_slice(k.as_str().as_bytes());
                header.extend_from_slice(b": ");
                header.extend_from_slice(v.as_bytes());
                header
            })
    }

    // According to RFC7578 Section 4.2, `filename*=` syntax is invalid.
    // See https://github.com/seanmonstar/reqwest/issues/419.
    fn format_filename(&self, filename: &str) -> String {
        let legal_filename = filename
            .replace("\\", "\\\\")
            .replace("\"", "\\\"")
            .replace("\r", "\\\r")
            .replace("\n", "\\\n");
        format!("filename=\"{}\"", legal_filename)
    }

    fn format_parameter(&self, name: &str, value: &str) -> String {
        let legal_value = match *self {
            PercentEncoding::PathSegment => {
                percent_encoding::utf8_percent_encode(value, PATH_SEGMENT_ENCODE_SET).to_string()
            }
            PercentEncoding::AttrChar => {
                percent_encoding::utf8_percent_encode(value, ATTR_CHAR_ENCODE_SET).to_string()
            }
            PercentEncoding::NoOp => value.to_string(),
        };
        if value.len() == legal_value.len() {
            // nothing has been percent encoded
            format!("{}=\"{}\"", name, value)
        } else {
            // something has been percent encoded
            format!("{}*=utf-8''{}", name, legal_value)
        }
    }
}

fn gen_boundary() -> String {
    let a = random();
    let b = random();
    let c = random();
    let d = random();

    format!("{:016x}-{:016x}-{:016x}-{:016x}", a, b, c, d)
}

// xor-shift
fn random() -> u64 {
    use std::cell::Cell;
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    use std::num::Wrapping;

    thread_local! {
        static RNG: Cell<Wrapping<u64>> = Cell::new(Wrapping(seed()));
    }

    fn seed() -> u64 {
        let seed = RandomState::new();

        let mut out = 0;
        let mut cnt = 0;
        while out == 0 {
            cnt += 1;
            let mut hasher = seed.build_hasher();
            hasher.write_usize(cnt);
            out = hasher.finish();
        }
        out
    }

    RNG.with(|rng| {
        let mut n = rng.get();
        debug_assert_ne!(n.0, 0);
        n ^= n >> 12;
        n ^= n << 25;
        n ^= n >> 27;
        rng.set(n);
        n.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    })
}
