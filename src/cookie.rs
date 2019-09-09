//! The cookies module contains types for working with request and response cookies.

use crate::{cookie_crate, Url};
use crate::header;
use std::borrow::Cow;
use std::fmt;
use std::time::SystemTime;

/// Convert a time::Tm time to SystemTime.
fn tm_to_systemtime(tm: time::Tm) -> SystemTime {
    let seconds = tm.to_timespec().sec;
    let duration = std::time::Duration::from_secs(seconds.abs() as u64);
    if seconds > 0 {
        SystemTime::UNIX_EPOCH + duration
    } else {
        SystemTime::UNIX_EPOCH - duration
    }
}

/// Error representing a parse failure of a 'Set-Cookie' header.
pub struct CookieParseError(cookie::ParseError);

impl<'a> fmt::Debug for CookieParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl<'a> fmt::Display for CookieParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for CookieParseError {}

/// A single HTTP cookie.
pub struct Cookie<'a>(cookie::Cookie<'a>);

impl<'a> fmt::Debug for Cookie<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Cookie<'static> {
    /// Construct a new cookie with the given name and value.
    pub fn new<N, V>(name: N, value: V) -> Self
    where
        N: Into<Cow<'static, str>>,
        V: Into<Cow<'static, str>>,
    {
        Cookie(cookie::Cookie::new(name, value))
    }
}

impl<'a> Cookie<'a> {
    fn parse(value: &'a crate::header::HeaderValue) -> Result<Cookie<'a>, CookieParseError> {
        std::str::from_utf8(value.as_bytes())
            .map_err(cookie::ParseError::from)
            .and_then(cookie::Cookie::parse)
            .map_err(CookieParseError)
            .map(Cookie)
    }

    pub(crate) fn into_inner(self) -> cookie::Cookie<'a> {
        self.0
    }

    /// The name of the cookie.
    pub fn name(&self) -> &str {
        self.0.name()
    }

    /// The value of the cookie.
    pub fn value(&self) -> &str {
        self.0.value()
    }

    /// Returns true if the 'HttpOnly' directive is enabled.
    pub fn http_only(&self) -> bool {
        self.0.http_only().unwrap_or(false)
    }

    /// Returns true if the 'Secure' directive is enabled.
    pub fn secure(&self) -> bool {
        self.0.secure().unwrap_or(false)
    }

    /// Returns true if  'SameSite' directive is 'Lax'.
    pub fn same_site_lax(&self) -> bool {
        self.0.same_site() == Some(cookie_crate::SameSite::Lax)
    }

    /// Returns true if  'SameSite' directive is 'Strict'.
    pub fn same_site_strict(&self) -> bool {
        self.0.same_site() == Some(cookie_crate::SameSite::Strict)
    }

    /// Returns the path directive of the cookie, if set.
    pub fn path(&self) -> Option<&str> {
        self.0.path()
    }

    /// Returns the domain directive of the cookie, if set.
    pub fn domain(&self) -> Option<&str> {
        self.0.domain()
    }

    /// Get the Max-Age information.
    pub fn max_age(&self) -> Option<std::time::Duration> {
        self.0
            .max_age()
            .map(|d| std::time::Duration::new(d.num_seconds() as u64, 0))
    }

    /// The cookie expiration time.
    pub fn expires(&self) -> Option<SystemTime> {
        self.0.expires().map(tm_to_systemtime)
    }
}

pub(crate) fn extract_response_cookies<'a>(
    headers: &'a hyper::HeaderMap,
) -> impl Iterator<Item = Result<Cookie<'a>, CookieParseError>> + 'a {
    headers
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|value| Cookie::parse(value))
}

/// An abstract cookie store that can be put into a client.
///
/// Use [`SimpleCookieStore`] when its API is sufficient, or a third-party crate like cookie_store if it's not.
pub trait CookieStore: Send + Sync + 'static {
    /// Returns cookies that should be attached to a request to this URL.
    fn get_request_cookies<'a>(&'a self, url: &Url) -> Box<dyn Iterator<Item = Cookie> + 'a>;
    /// Add cookies returned from the server as a response to a request with the given URL.
    fn store_response_cookies(&mut self, cookies: &mut dyn Iterator<Item = Cookie>, url: &Url);
}

/// A default cookie store with no additional functions.
///
/// To create an instance, use the Default impl.
#[derive(Default)]
pub struct SimpleCookieStore(pub(crate) cookie_store::CookieStore);

impl CookieStore for SimpleCookieStore {
    fn get_request_cookies<'a>(&'a self, url: &Url) -> Box<dyn Iterator<Item = Cookie> + 'a> {
        Box::new(
            self.0
                .get_request_cookies(url)
                .map(|cookie| Cookie(cookie.clone().into_owned())),
        )
    }

    fn store_response_cookies(&mut self, cookies: &mut dyn Iterator<Item = Cookie>, url: &Url) {
        self.0
            .store_response_cookies(cookies.map(|cookie| cookie.into_inner().into_owned()), url);
    }
}

impl fmt::Debug for SimpleCookieStore {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}
