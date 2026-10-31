//! The side table that gives a 64-bit element its Ruby string back.

use std::collections::HashMap;

use magnus::{Error, RArray, Ruby};

use crate::hash::hash64;
use crate::{FIELD_LEN, field};

/// What each element the core holds was hashed from.
pub type Names = HashMap<u64, String>;

/// Appends one record per named element to `blob`: a 64-bit little-endian byte
/// length, then that many UTF-8 bytes.
pub fn write(blob: &mut Vec<u8>, names: &Names, elements: impl Iterator<Item = u64>) {
    for name in elements.filter_map(|element| names.get(&element)) {
        blob.extend_from_slice(&field(name.len()));
        blob.extend_from_slice(name.as_bytes());
    }
}

/// Reads the records that follow a core frame back into a side table.
pub fn read(ruby: &Ruby, mut records: &[u8]) -> Result<Names, Error> {
    let mut names = Names::new();
    while !records.is_empty() {
        let Some((length, tail)) = records.split_first_chunk::<FIELD_LEN>() else {
            return Err(rejected(ruby, "ends inside a record header"));
        };
        let Ok(length) = usize::try_from(u64::from_le_bytes(*length)) else {
            return Err(rejected(ruby, "names a key this platform cannot hold"));
        };
        let Some((name, tail)) = tail.split_at_checked(length) else {
            return Err(rejected(ruby, "ends inside a key"));
        };
        let Ok(name) = str::from_utf8(name) else {
            return Err(rejected(ruby, "holds a key that is not UTF-8"));
        };
        names.insert(hash64(name.as_bytes()), name.to_owned());
        records = tail;
    }
    Ok(names)
}

/// Checks that every element the core holds has a string to come back as.
pub fn complete(
    ruby: &Ruby,
    names: &Names,
    mut elements: impl Iterator<Item = u64>,
) -> Result<(), Error> {
    if elements.all(|element| names.contains_key(&element)) {
        Ok(())
    } else {
        Err(rejected(ruby, "does not name every element"))
    }
}

/// The strings `elements` came from, as a Ruby array in the order the core
/// reported them.
pub fn members(ruby: &Ruby, names: &Names, elements: &[u64]) -> Result<RArray, Error> {
    let members = ruby.ary_new_capa(elements.len());
    for element in elements {
        members.push(ruby.str_new(&named(ruby, names, *element)?))?;
    }
    Ok(members)
}

/// The string an element came from.
fn named(ruby: &Ruby, names: &Names, element: u64) -> Result<String, Error> {
    names.get(&element).cloned().ok_or_else(|| {
        Error::new(
            ruby.exception_runtime_error(),
            "an element lost the key it was hashed from",
        )
    })
}

fn rejected(ruby: &Ruby, reason: &str) -> Error {
    Error::new(
        ruby.exception_arg_error(),
        format!("the blob's key table {reason}"),
    )
}
