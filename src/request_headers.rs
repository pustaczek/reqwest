use http::{HeaderMap};

pub(crate) fn replace_headers(dst: &mut HeaderMap, src: HeaderMap) {
    // IntoIter of HeaderMap yields (Option<HeaderName>, HeaderValue).
    // The first time a name is yielded, it will be Some(name), and if
    // there are more values with the same name, the next yield will be
    // None.
    //
    // TODO: a complex exercise would be to optimize this to only
    // require 1 hash/lookup of the key, but doing something fancy
    // with header::Entry...

    let mut prev_name = None;
    for (key, value) in src {
        match key {
            Some(key) => {
                dst.insert(key.clone(), value);
                prev_name = Some(key);
            }
            None => match prev_name {
                Some(ref key) => {
                    dst.append(key.clone(), value);
                }
                None => unreachable!("HeaderMap::into_iter yielded None first"),
            },
        }
    }
}
