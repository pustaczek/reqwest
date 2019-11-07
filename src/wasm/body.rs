use std::fmt;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use futures_core::Stream;
use futures_util::TryStreamExt;

/// An asynchronous request body.
pub struct Body {
    inner: Inner,
}

// The `Stream` trait isn't stable, so the impl isn't public.
pub(crate) struct ImplStream(Body);

enum Inner {
    Reusable(Bytes),
    Streaming {
        body: Pin<
            Box<
                dyn Stream<Item = Result<Bytes, crate::Error>>
                    + Send
                    + Sync,
            >,
        >,
    },
}

impl Body {
    pub(crate) fn stream<S>(stream: S) -> Body
    where
        S: futures_core::stream::TryStream + Send + Sync + 'static,
        S::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
        Bytes: From<S::Ok>,
    {
        use futures_util::TryStreamExt;

        let body = Box::pin(
            stream.map_ok(Bytes::from).map_err(|e| crate::Error::new(crate::error::Kind::Body, Some(Into::into(e)))),
        );
        Body {
            inner: Inner::Streaming {
                body,
            },
        }
    }
    pub(crate) fn empty() -> Body {
        Body::reusable(Bytes::new())
    }

    pub(crate) fn reusable(chunk: Bytes) -> Body {
        Body {
            inner: Inner::Reusable(chunk),
        }
    }

    pub(crate) fn into_stream(self) -> ImplStream {
        ImplStream(self)
    }

    pub(crate) fn content_length(&self) -> Option<u64> {
        match self.inner {
            Inner::Reusable(ref bytes) => Some(bytes.len() as u64),
            Inner::Streaming { .. } => None,
        }
    }

    pub(crate) async fn read_into_bytes(self) -> Result<Vec<u8>, crate::Error> {
        match self.inner {
            Inner::Reusable(ref bytes) => Ok(bytes.as_ref().to_owned()),
            Inner::Streaming { body } => {
                let mut buf = Vec::new();
                body.try_for_each(|chunk| {
                    buf.extend_from_slice(&chunk);
                    futures_util::future::ready(Ok(()))
                }).await?;
                Ok(buf)
            }
        }
    }
}

impl From<Bytes> for Body {
    #[inline]
    fn from(bytes: Bytes) -> Body {
        Body::reusable(bytes)
    }
}

impl From<Vec<u8>> for Body {
    #[inline]
    fn from(vec: Vec<u8>) -> Body {
        Body::reusable(vec.into())
    }
}

impl From<&'static [u8]> for Body {
    #[inline]
    fn from(s: &'static [u8]) -> Body {
        Body::reusable(Bytes::from_static(s))
    }
}

impl From<String> for Body {
    #[inline]
    fn from(s: String) -> Body {
        Body::reusable(s.into())
    }
}

impl From<&'static str> for Body {
    #[inline]
    fn from(s: &'static str) -> Body {
        s.as_bytes().into()
    }
}

impl fmt::Debug for Body {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Body").finish()
    }
}

// ===== impl ImplStream =====

impl Stream for ImplStream {
    type Item = Result<Bytes, crate::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        match &mut self.get_mut().0.inner {
            Inner::Reusable(bytes) if !bytes.is_empty() => Poll::Ready(Some(Ok(std::mem::replace(bytes, Bytes::new())))),
            Inner::Reusable(_) => Poll::Ready(None),
            Inner::Streaming { body } => Stream::poll_next(body.as_mut(), cx),
        }
    }
}
