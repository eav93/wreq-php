//! Conversion helpers between PHP values and `wreq` types.

use ext_php_rs::types::ZendHashTable;
use wreq::header::{HeaderMap, HeaderName, HeaderValue};

use crate::error::{Error, Result};

/// Builds a `HeaderMap` from a PHP associative array (`name => value`).
///
/// Invalid header names/values are reported as an error rather than silently
/// dropped, so callers get clear feedback on a bad header.
///
/// Uses `insert` rather than `append` so two PHP keys differing only in case
/// (e.g. `Content-Type` and `content-type`) collapse to a single header on
/// the wire. The PHP layer already deduplicates case-insensitively, but this
/// defends against callers that bypass `PendingRequest` and pass a raw array
/// to `Client::request`.
pub fn headers_from_table(table: &ZendHashTable) -> Result<HeaderMap> {
    let mut map = HeaderMap::new();
    for (key, value) in table.iter() {
        let name = key.to_string();
        let val = value
            .str()
            .ok_or_else(|| Error::other(format!("header '{name}' must be a string")))?;
        let header_name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| Error::other(format!("invalid header name: '{name}'")))?;
        let header_value = HeaderValue::from_str(val)
            .map_err(|_| Error::other(format!("invalid value for header '{name}'")))?;
        map.insert(header_name, header_value);
    }
    Ok(map)
}
