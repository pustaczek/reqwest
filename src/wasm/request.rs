use std::fmt;

use http::{Method, header::HeaderName, HeaderMap, HeaderValue, HttpTryFrom};
use url::Url;

use super::{Body, Client, Response};

/// A request which can be executed with `Client::execute()`.
pub struct Request {
    method: Method,
    url: Url,
    headers: HeaderMap,
    body: Option<Body>,
}

/// A builder to construct the properties of a `Request`.
pub struct RequestBuilder {
    client: Client,
    request: crate::Result<Request>,
}

impl Request {
    pub(super) fn new(method: Method, url: Url) -> Self {
        Request {
            method,
            url,
            headers: HeaderMap::new(),
            body: None,
        }
    }

    /// Get the method.
    #[inline]
    pub fn method(&self) -> &Method {
        &self.method
    }

    /// Get a mutable reference to the method.
    #[inline]
    pub fn method_mut(&mut self) -> &mut Method {
        &mut self.method
    }

    /// Get the url.
    #[inline]
    pub fn url(&self) -> &Url {
        &self.url
    }

    /// Get a mutable reference to the url.
    #[inline]
    pub fn url_mut(&mut self) -> &mut Url {
        &mut self.url
    }

    /// Get the headers.
    #[inline]
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// Get a mutable reference to the headers.
    #[inline]
    pub fn headers_mut(&mut self) -> &mut HeaderMap {
        &mut self.headers
    }

    /// Get the body.
    #[inline]
    pub fn body(&self) -> Option<&Body> {
        self.body.as_ref()
    }

    /// Get a mutable reference to the body.
    #[inline]
    pub fn body_mut(&mut self) -> &mut Option<Body> {
        &mut self.body
    }
}

impl RequestBuilder {
    pub(super) fn new(client: Client, request: crate::Result<Request>) -> RequestBuilder {
        RequestBuilder { client, request }
    }

    /// Add a `Header` to this Request.
    pub fn header<K, V>(mut self, key: K, value: V) -> RequestBuilder
    where
        HeaderName: HttpTryFrom<K>,
        HeaderValue: HttpTryFrom<V>,
    {
        let mut error = None;
        if let Ok(ref mut req) = self.request {
            match <HeaderName as HttpTryFrom<K>>::try_from(key) {
                Ok(key) => match <HeaderValue as HttpTryFrom<V>>::try_from(value) {
                    Ok(value) => {
                        req.headers_mut().append(key, value);
                    }
                    Err(e) => error = Some(crate::error::builder(e.into())),
                },
                Err(e) => error = Some(crate::error::builder(e.into())),
            };
        }
        if let Some(err) = error {
            self.request = Err(err);
        }
        self
    }

    /// Add a set of Headers to the existing ones on this Request.
    ///
    /// The headers will be merged in to any already set.
    pub fn headers(mut self, headers: crate::header::HeaderMap) -> RequestBuilder {
        if let Ok(ref mut req) = self.request {
            crate::request_headers::replace_headers(req.headers_mut(), headers);
        }
        self
    }

    /// Set the request body.
    pub fn body<T: Into<Body>>(mut self, body: T) -> RequestBuilder {
        if let Ok(ref mut req) = self.request {
            req.body = Some(body.into());
        }
        self
    }

    /// Constructs the Request and sends it to the target URL, returning a
    /// future Response.
    ///
    /// # Errors
    ///
    /// This method fails if there was an error while sending request.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use reqwest::Error;
    /// #
    /// # async fn run() -> Result<(), Error> {
    /// let response = reqwest::Client::new()
    ///     .get("https://hyper.rs")
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn send(self) -> crate::Result<Response> {
        let req = self.request?;
        self.client.execute_request(req).await
    }
}

impl fmt::Debug for Request {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt_request_fields(&mut f.debug_struct("Request"), self).finish()
    }
}

impl fmt::Debug for RequestBuilder {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut builder = f.debug_struct("RequestBuilder");
        match self.request {
            Ok(ref req) => fmt_request_fields(&mut builder, req).finish(),
            Err(ref err) => builder.field("error", err).finish(),
        }
    }
}

fn fmt_request_fields<'a, 'b>(
    f: &'a mut fmt::DebugStruct<'a, 'b>,
    req: &Request,
) -> &'a mut fmt::DebugStruct<'a, 'b> {
    f.field("method", &req.method)
        .field("url", &req.url)
        .field("headers", &req.headers)
}
