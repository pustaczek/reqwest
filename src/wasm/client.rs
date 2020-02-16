use http::{header::{ACCEPT, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, Entry, LOCATION, REFERER, TRANSFER_ENCODING, USER_AGENT}, HeaderMap, HeaderValue, Method, StatusCode};
use js_sys::Uint8Array;
use log::debug;
use std::{future::Future, str, sync::{Arc, RwLock}};
use url::Url;
use wasm_bindgen::UnwrapThrowExt as _;
use wasm_bindgen::JsCast;

use super::{Body, Request, RequestBuilder, Response};
use crate::{cookie, DEFAULT_USER_AGENT, into_url::{expect_uri, try_uri}, redirect::{self, remove_sensitive_headers, RedirectPolicy}, IntoUrl};

/// dox
#[derive(Clone, Debug)]
pub struct Client(Arc<ClientState>);

#[derive(Debug)]
struct ClientState {
    #[cfg(feature = "cookies")]
    cookie_store: Option<RwLock<cookie::CookieStore>>,
    headers: HeaderMap,
}

/// dox
#[derive(Debug)]
pub struct ClientBuilder {
    headers: HeaderMap,
    #[cfg(feature = "cookies")]
    cookie_store: Option<cookie::CookieStore>,
}

impl Client {
    /// dox
    pub fn new() -> Self {
        Client::builder().build().unwrap_throw()
    }

    /// dox
    pub fn builder() -> ClientBuilder {
        ClientBuilder::new()
    }

    /// Convenience method to make a `GET` request to a URL.
    ///
    /// # Errors
    ///
    /// This method fails whenever supplied `Url` cannot be parsed.
    pub fn get<U: IntoUrl>(&self, url: U) -> RequestBuilder {
        self.request(Method::GET, url)
    }

    /// Convenience method to make a `POST` request to a URL.
    ///
    /// # Errors
    ///
    /// This method fails whenever supplied `Url` cannot be parsed.
    pub fn post<U: IntoUrl>(&self, url: U) -> RequestBuilder {
        self.request(Method::POST, url)
    }

    /// Convenience method to make a `PUT` request to a URL.
    ///
    /// # Errors
    ///
    /// This method fails whenever supplied `Url` cannot be parsed.
    pub fn put<U: IntoUrl>(&self, url: U) -> RequestBuilder {
        self.request(Method::PUT, url)
    }

    /// Convenience method to make a `PATCH` request to a URL.
    ///
    /// # Errors
    ///
    /// This method fails whenever supplied `Url` cannot be parsed.
    pub fn patch<U: IntoUrl>(&self, url: U) -> RequestBuilder {
        self.request(Method::PATCH, url)
    }

    /// Convenience method to make a `DELETE` request to a URL.
    ///
    /// # Errors
    ///
    /// This method fails whenever supplied `Url` cannot be parsed.
    pub fn delete<U: IntoUrl>(&self, url: U) -> RequestBuilder {
        self.request(Method::DELETE, url)
    }

    /// Convenience method to make a `HEAD` request to a URL.
    ///
    /// # Errors
    ///
    /// This method fails whenever supplied `Url` cannot be parsed.
    pub fn head<U: IntoUrl>(&self, url: U) -> RequestBuilder {
        self.request(Method::HEAD, url)
    }

    /// Start building a `Request` with the `Method` and `Url`.
    ///
    /// Returns a `RequestBuilder`, which will allow setting headers and
    /// request body before sending.
    ///
    /// # Errors
    ///
    /// This method fails whenever supplied `Url` cannot be parsed.
    pub fn request<U: IntoUrl>(&self, method: Method, url: U) -> RequestBuilder {
        let req = url.into_url().map(move |url| Request::new(method, url));
        RequestBuilder::new(self.clone(), req)
    }

    /// dox
    pub fn cookies(&self) -> Option<&RwLock<cookie::CookieStore>> {
        self.0.cookie_store.as_ref()
    }

    pub(super) fn execute_request(
        &self,
        req: Request,
    ) -> impl Future<Output = crate::Result<Response>> {
        fetch(self.clone(), req)
    }
}

async fn fetch(client: Client, req: Request) -> crate::Result<Response> {
    let (mut method, mut url, mut headers, body) = req.pieces();

    // insert default headers in the request headers
    // without overwriting already appended headers.
    for (key, value) in &client.0.headers {
        if let Ok(Entry::Vacant(entry)) = headers.entry(key) {
            entry.insert(value.clone());
        }
    }

    // Add cookies from the cookie store.
    #[cfg(feature = "cookies")]
    {
        if let Some(cookie_store_wrapper) = client.0.cookie_store.as_ref() {
            if headers.get(crate::header::COOKIE).is_none() {
                let cookie_store = cookie_store_wrapper.read().unwrap();
                add_cookie_header(&mut headers, &cookie_store, &url);
            }
        }
    }

    let (mut body, original_body) = match body {
        Some(body) => {
            let (reusable, body) = body.try_reuse();
            (Some(reusable), Some(body))
        }
        None => (None, None),
    };

    let mut urls = Vec::new();

    let mut res_future = run_fetch(method.clone(), url.clone(), headers.clone(), original_body);
    loop {
        let res = res_future.await?;

        #[cfg(feature = "cookies")]
        {
            if let Some(store_wrapper) = client.0.cookie_store.as_ref() {
                let mut store = store_wrapper.write().unwrap();
                let cookies = cookie::extract_response_cookies(&res.headers())
                    .filter_map(|res| res.ok())
                    .map(|cookie| cookie.into_inner().into_owned());
                store.0.store_response_cookies(cookies, &url);
            }
        }
        let should_redirect = match res.status() {
            StatusCode::MOVED_PERMANENTLY | StatusCode::FOUND | StatusCode::SEE_OTHER => {
                body = None;
                for header in &[
                    TRANSFER_ENCODING,
                    CONTENT_ENCODING,
                    CONTENT_TYPE,
                    CONTENT_LENGTH,
                ] {
                    headers.remove(header);
                }

                match method {
                    Method::GET | Method::HEAD => {}
                    _ => {
                        method = Method::GET;
                    }
                }
                true
            }
            StatusCode::TEMPORARY_REDIRECT | StatusCode::PERMANENT_REDIRECT => {
                match body {
                    Some(Some(_)) | None => true,
                    Some(None) => false,
                }
            }
            _ => false,
        };
        if should_redirect {
            let loc = res.headers().get(LOCATION).and_then(|val| {
                let loc = (|| -> Option<Url> {
                    // Some sites may send a utf-8 Location header,
                    // even though we're supposed to treat those bytes
                    // as opaque, we'll check specifically for utf8.
                    url.join(str::from_utf8(val.as_bytes()).ok()?).ok()
                })();

                // Check that the `url` is also a valid `http::Uri`.
                //
                // If not, just log it and skip the redirect.
                let loc = loc.and_then(|url| {
                    if try_uri(&url).is_some() {
                        Some(url)
                    } else {
                        None
                    }
                });

                if loc.is_none() {
                    debug!("Location header had invalid URI: {:?}", val);
                }
                loc
            });
            if let Some(loc) = loc {
                // TODO: if client.0.referer {
                if let Some(referer) = make_referer(&loc, &url) {
                    headers.insert(REFERER, referer);
                }

                // let url = url.clone();
                urls.push(url.clone());
                // TODO: client.0.redirect_policy
                let action = RedirectPolicy::default()
                    .check(res.status(), &loc, &urls);

                match action {
                    redirect::Action::Follow => {
                        url = loc;

                        debug!("redirecting to {:?} '{}'", method, url);
                        remove_sensitive_headers(&mut headers, &url, &urls);
                        let body = match body {
                            Some(Some(ref body)) => Some(Body::reusable(body.clone())),
                            _ => None,
                        };
                        // Add cookies from the cookie store.
                        #[cfg(feature = "cookies")]
                        {
                            if let Some(cookie_store_wrapper) =
                                client.0.cookie_store.as_ref()
                            {
                                let cookie_store = cookie_store_wrapper.read().unwrap();
                                add_cookie_header(&mut headers, &cookie_store, &url);
                            }
                        }
                        res_future = run_fetch(method.clone(), url.clone(), headers.clone(), body);
                        continue;
                    }
                    redirect::Action::Stop => {
                        debug!("redirect_policy disallowed redirection to '{}'", loc);
                    }
                    redirect::Action::LoopDetected => {
                        return Err(crate::error::loop_detected(url.clone()));
                    }
                    redirect::Action::TooManyRedirects => {
                        return Err(crate::error::too_many_redirects(
                            url.clone(),
                        ));
                    }
                }
            }
        }
        let res = Response::new(res, url.clone());
        return Ok(res);
    }
}

async fn run_fetch(method: Method, url: Url, headers: HeaderMap, body: Option<Body>) -> crate::Result<http::Response<web_sys::Response>> {
    let js_req = build_fetch_request(method, &url, headers, body).await?;
    // Await the fetch() promise
    let p = web_sys::window()
        .expect("window should exist")
        .fetch_with_request(&js_req);
    let js_resp = super::promise::<web_sys::Response>(p)
        .await
        .map_err(crate::error::request)?;
    let resp = convert_fetch_response(js_resp)?;
    Ok(resp)
}

async fn build_fetch_request(method: Method, url: &Url, headers: HeaderMap, body: Option<Body>) -> crate::Result<web_sys::Request> {
    let mut init = web_sys::RequestInit::new();
    init.method(method.as_str());
    init.headers(&build_fetch_headers(headers)?.into());
    init.redirect(web_sys::RequestRedirect::Manual);
    if let Some(body) = body {
        let body_bytes = body.read_into_bytes().await?;
        let body_array: Uint8Array = body_bytes.as_slice().into();
        init.body(Some(&body_array.into()));
    }
    let js_req = web_sys::Request::new_with_str_and_init(url.as_str(), &init)
        .map_err(crate::error::wasm)
        .map_err(crate::error::builder)?;
    Ok(js_req)
}

fn build_fetch_headers(headers: HeaderMap) -> crate::Result<web_sys::Headers> {
    let js_headers = web_sys::Headers::new()
        .map_err(crate::error::wasm)
        .map_err(crate::error::builder)?;
    for (name, value) in headers {
        js_headers
            .append(
                name.unwrap().as_str(),
                value.to_str().map_err(crate::error::builder)?,
            )
            .map_err(crate::error::wasm)
            .map_err(crate::error::builder)?;
    }
    Ok(js_headers)
}

fn convert_fetch_response(js_resp: web_sys::Response) -> crate::Result<http::Response<web_sys::Response>> {
    let mut resp = http::Response::builder();
    resp.status(js_resp.status());
    for (header_name, header_value) in convert_fetch_headers(&js_resp) {
        resp.header(&header_name, &header_value);
    }
    Ok(resp.body(js_resp).map_err(crate::error::request)?)
}

fn convert_fetch_headers(js_resp: &web_sys::Response) -> impl Iterator<Item = (String, String)> {
    let js_headers = js_resp.headers();
    let js_headers_raw = js_sys::Reflect::get(&js_headers, &js_sys::JsString::from("raw"))
        .expect_throw("node-fetch .headers.raw does not exist")
        .dyn_into::<js_sys::Function>()
        .expect_throw("node-fetch .headers.raw is not a function")
        .call0(&js_headers)
        .expect_throw("node-fetch .headers.raw() call failed");
    let header_names = js_sys::Reflect::own_keys(&js_headers_raw)
        .expect_throw("node-fetch .headers.raw() is not an object");
    header_names
        .to_vec()
        .into_iter()
        .flat_map(|header_name| {
            let header_name = header_name
                .dyn_into::<js_sys::JsString>()
                .expect_throw("node-fetch raw header name is not a string");
            let header_values = js_sys::Reflect::get(&js_headers_raw, &header_name)
                .expect_throw("node-fetch raw header contains keys it does not contain");
            let header_values = header_values
                .dyn_into::<js_sys::Array>()
                .expect_throw("node-fetch raw header values are not an array");
            let header_name = String::from(&header_name);
            header_values
                .to_vec()
                .into_iter()
                .map(|header_value| {
                    let header_value = header_value
                        .dyn_into::<js_sys::JsString>()
                        .expect_throw("node-fetch raw header value is not a string");
                    (header_name.clone(), header_value.into())
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>()
        .into_iter()
}

#[cfg(feature = "cookies")]
fn add_cookie_header(headers: &mut HeaderMap, cookie_store: &cookie::CookieStore, url: &Url) {
    let header = cookie_store
        .0
        .get_request_cookies(url)
        .map(|c| format!("{}={}", c.name(), c.value()))
        .collect::<Vec<_>>()
        .join("; ");
    if !header.is_empty() {
        headers.insert(
            crate::header::COOKIE,
            HeaderValue::from_bytes(header.as_bytes()).unwrap(),
        );
    }
}

fn make_referer(next: &Url, previous: &Url) -> Option<HeaderValue> {
    if next.scheme() == "http" && previous.scheme() == "https" {
        return None;
    }

    let mut referer = previous.clone();
    let _ = referer.set_username("");
    let _ = referer.set_password(None);
    referer.set_fragment(None);
    referer.as_str().parse().ok()
}

// ===== impl ClientBuilder =====

impl ClientBuilder {
    /// dox
    pub fn new() -> Self {
        let mut headers: HeaderMap<HeaderValue> = HeaderMap::with_capacity(2);
        headers.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_USER_AGENT));
        headers.insert(ACCEPT, HeaderValue::from_static("*/*"));
        ClientBuilder {
            headers,
            #[cfg(feature = "cookies")]
            cookie_store: None,
        }
    }

    /// dox
    pub fn build(self) -> Result<Client, crate::Error> {
        Ok(Client(Arc::new(ClientState {
            #[cfg(feature = "cookies")]
            cookie_store: self.cookie_store.map(RwLock::new),
            headers: self.headers,
        })))
    }

    /// dox
    pub fn default_headers(mut self, headers: HeaderMap) -> ClientBuilder {
        for (key, value) in headers.iter() {
            self.headers.insert(key, value.clone());
        }
        self
    }

    /// dox
    #[cfg(feature = "cookies")]
    pub fn cookie_store(mut self, enable: bool) -> ClientBuilder {
        self.cookie_store = if enable {
            Some(cookie::CookieStore::default())
        } else {
            None
        };
        self
    }
}
