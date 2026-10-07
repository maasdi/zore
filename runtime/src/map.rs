//! Keys are `kind` raw bytes or a string matched by content; each value has a stable address.

use std::collections::HashMap;

use super::alloc::{zore_alloc, zore_free};

const STRING_KEY: i32 = 16;

pub struct Map {
    index: HashMap<Vec<u8>, usize>,
    entries: Vec<Entry>,
    value_size: i64,
    text_keys: bool,
}

struct Entry {
    key: Vec<u8>,
    /// The key as the program passed it; a string key keeps its descriptor and owns its text.
    raw: [u8; 16],
    value: *mut u8,
}

/// The key's in-memory representation, which is at most 16 bytes.
///
/// # Safety
/// `key` must point to a live key of the layout `kind` names.
unsafe fn raw_key(kind: i32, key: *const u8) -> [u8; 16] {
    let size = if kind == STRING_KEY {
        16
    } else {
        kind as usize
    };
    let mut raw = [0; 16];
    // SAFETY: the caller supplies `size` readable bytes.
    unsafe { std::ptr::copy_nonoverlapping(key, raw.as_mut_ptr(), size.min(16)) };
    raw
}

/// The canonical bytes of the key at `key`.
///
/// # Safety
/// `key` must point to a live key of the layout `kind` names.
fn text_of(raw: &[u8; 16]) -> (*const u8, usize) {
    let data = usize::from_ne_bytes(raw[..8].try_into().unwrap()) as *const u8;
    let len = i64::from_ne_bytes(raw[8..].try_into().unwrap());
    (data, usize::try_from(len).unwrap_or(0))
}

fn retain_key(map: &Map, raw: &[u8; 16]) {
    if map.text_keys {
        let (data, len) = text_of(raw);
        super::string::retain(data, len);
    }
}

fn release_key(map: &Map, raw: &[u8; 16]) {
    if map.text_keys {
        let (data, len) = text_of(raw);
        super::string::release(data, len);
    }
}

unsafe fn key_bytes(kind: i32, key: *const u8) -> Vec<u8> {
    if kind == STRING_KEY {
        // SAFETY: a string key is a `{ ptr, i64 }` descriptor of live bytes.
        let (data, len) = unsafe { (*(key as *const *const u8), *(key as *const i64).add(1)) };
        return unsafe { super::bytes(data, len) }.to_vec();
    }
    let size = usize::try_from(kind).unwrap_or_else(|_| super::panic::fail(b"invalid map key"));
    // SAFETY: the caller supplies `kind` readable bytes.
    unsafe { std::slice::from_raw_parts(key, size) }.to_vec()
}

/// The stored value for the key, or null if absent (or the map is empty).
///
/// # Safety
/// `map` must be null or a live map; `key` must match `kind`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_map_find(map: *const Map, kind: i32, key: *const u8) -> *mut u8 {
    // SAFETY: guaranteed by the caller.
    let Some(map) = (unsafe { map.as_ref() }) else {
        return std::ptr::null_mut();
    };
    let key = unsafe { key_bytes(kind, key) };
    map.index
        .get(&key)
        .map_or(std::ptr::null_mut(), |&index| map.entries[index].value)
}

/// The key must be absent; returns uninitialized storage for its value.
///
/// # Safety
/// `slot` must point to a null or live map pointer; `key` must match `kind`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_map_insert(
    slot: *mut *mut Map,
    kind: i32,
    key: *const u8,
    value_size: i64,
) -> *mut u8 {
    // SAFETY: guaranteed by the caller.
    let map = unsafe {
        if (*slot).is_null() {
            *slot = Box::into_raw(Box::new(Map {
                index: HashMap::new(),
                entries: Vec::new(),
                value_size,
                text_keys: kind == STRING_KEY,
            }));
        }
        &mut **slot
    };
    let raw = unsafe { raw_key(kind, key) };
    let key = unsafe { key_bytes(kind, key) };
    retain_key(map, &raw);
    let value = zore_alloc(value_size);
    map.index.insert(key.clone(), map.entries.len());
    map.entries.push(Entry { key, raw, value });
    value
}

/// Moves the value's bytes to `out` and returns whether the key was present.
///
/// # Safety
/// `map` must be null or a live map; `key` must match `kind`; `out` must
/// have room for one value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_map_detach(
    map: *mut Map,
    kind: i32,
    key: *const u8,
    out: *mut u8,
) -> bool {
    // SAFETY: guaranteed by the caller.
    let Some(map) = (unsafe { map.as_mut() }) else {
        return false;
    };
    let key = unsafe { key_bytes(kind, key) };
    let Some(index) = map.index.remove(&key) else {
        return false;
    };
    let removed = map.entries.swap_remove(index);
    release_key(map, &removed.raw);
    let value = removed.value;
    if let Some(moved) = map.entries.get(index) {
        map.index.insert(moved.key.clone(), index);
    }
    let size = usize::try_from(map.value_size).unwrap_or(0);
    // SAFETY: `value` holds one initialized value and `out` has room for it.
    unsafe {
        std::ptr::copy_nonoverlapping(value, out, size);
        zore_free(value, map.value_size);
    }
    true
}

/// The number of entries; zero for the empty (null) map.
///
/// # Safety
/// `map` must be null or a live map.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_map_len(map: *const Map) -> i64 {
    // SAFETY: guaranteed by the caller.
    unsafe { map.as_ref() }.map_or(0, |map| map.entries.len() as i64)
}

/// The value of entry `index`, in an unspecified but stable order.
///
/// # Safety
/// `map` must be live and `index` below its length.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_map_value_at(map: *const Map, index: i64) -> *mut u8 {
    // SAFETY: guaranteed by the caller.
    let map = unsafe { &*map };
    map.entries[usize::try_from(index).unwrap_or(usize::MAX)].value
}

/// The key of entry `index` as the program stored it.
///
/// # Safety
/// `map` must be live and `index` below its length.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_map_key_at(map: *const Map, index: i64) -> *const u8 {
    // SAFETY: guaranteed by the caller.
    let map = unsafe { &*map };
    map.entries[usize::try_from(index).unwrap_or(usize::MAX)]
        .raw
        .as_ptr()
}

/// A map with the same keys in the same order and uninitialized values; null when empty.
///
/// # Safety
/// `map` must be null or a live map.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_map_clone_shape(map: *const Map) -> *mut Map {
    // SAFETY: guaranteed by the caller.
    let Some(map) = (unsafe { map.as_ref() }) else {
        return std::ptr::null_mut();
    };
    if map.entries.is_empty() {
        return std::ptr::null_mut();
    }
    let entries = map
        .entries
        .iter()
        .map(|entry| {
            retain_key(map, &entry.raw);
            Entry {
                key: entry.key.clone(),
                raw: entry.raw,
                value: zore_alloc(map.value_size),
            }
        })
        .collect();
    Box::into_raw(Box::new(Map {
        index: map.index.clone(),
        entries,
        value_size: map.value_size,
        text_keys: map.text_keys,
    }))
}

/// Frees a map whose values have already been destroyed; null is a no-op.
///
/// # Safety
/// `map` must be null or a live map that is not used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_map_free(map: *mut Map) {
    if map.is_null() {
        return;
    }
    // SAFETY: the map came from `Box::into_raw` in `zore_map_insert`.
    let map = unsafe { Box::from_raw(map) };
    for entry in &map.entries {
        release_key(&map, &entry.raw);
        // SAFETY: each value came from `zore_alloc(map.value_size)`.
        unsafe { zore_free(entry.value, map.value_size) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn insert(map: &mut *mut Map, key: i64, value: i64) {
        // SAFETY: `map` is null or live; keys and values are 8 bytes.
        unsafe {
            let slot = zore_map_insert(map, 8, (&key as *const i64).cast(), 8);
            *(slot as *mut i64) = value;
        }
    }

    #[test]
    fn entries_insert_find_detach_and_free() {
        let _serial = crate::string::serial();
        let mut map: *mut Map = std::ptr::null_mut();
        // SAFETY: the empty map is null, which every function accepts.
        unsafe {
            assert!(zore_map_find(map, 8, (&1i64 as *const i64).cast()).is_null());
            assert_eq!(zore_map_len(map), 0);
        }
        for key in 0..20 {
            insert(&mut map, key, key * 10);
        }
        // SAFETY: `map` is live; keys are 8 bytes; `out` holds one value.
        unsafe {
            assert_eq!(zore_map_len(map), 20);
            let found = zore_map_find(map, 8, (&7i64 as *const i64).cast());
            assert_eq!(*(found as *const i64), 70);
            let mut out = 0i64;
            assert!(zore_map_detach(
                map,
                8,
                (&3i64 as *const i64).cast(),
                (&mut out as *mut i64).cast()
            ));
            assert_eq!(out, 30);
            assert!(!zore_map_detach(
                map,
                8,
                (&3i64 as *const i64).cast(),
                (&mut out as *mut i64).cast()
            ));
            for key in (0..20).filter(|&key| key != 3) {
                let found = zore_map_find(map, 8, (&key as *const i64).cast());
                assert_eq!(*(found as *const i64), key * 10);
            }
            zore_map_free(map);
        }
    }

    #[test]
    fn clone_shape_keeps_keys_and_order_with_fresh_values() {
        let _serial = crate::string::serial();
        let mut map: *mut Map = std::ptr::null_mut();
        // SAFETY: the empty map is null, which every function accepts.
        unsafe {
            assert!(zore_map_clone_shape(map).is_null());
        }
        for key in 0..5 {
            insert(&mut map, key, key * 10);
        }
        // SAFETY: `map` is live; the copy's values are written before reading.
        unsafe {
            let copy = zore_map_clone_shape(map);
            assert_eq!(zore_map_len(copy), 5);
            for index in 0..5 {
                let source = zore_map_value_at(map, index);
                let target = zore_map_value_at(copy, index);
                assert_ne!(source, target);
                *(target as *mut i64) = *(source as *const i64) + 1;
            }
            let found = zore_map_find(copy, 8, (&3i64 as *const i64).cast());
            assert_eq!(*(found as *const i64), 31);
            let original = zore_map_find(map, 8, (&3i64 as *const i64).cast());
            assert_eq!(*(original as *const i64), 30);
            zore_map_free(copy);
            zore_map_free(map);
        }
    }

    #[test]
    fn string_keys_match_by_content() {
        let _serial = crate::string::serial();
        let mut map: *mut Map = std::ptr::null_mut();
        let first = b"key".to_vec();
        let second = b"key".to_vec();
        let key = |bytes: &Vec<u8>| (bytes.as_ptr(), bytes.len() as i64);
        // SAFETY: string keys are live `{ ptr, i64 }` descriptors.
        unsafe {
            let descriptor = key(&first);
            let slot = zore_map_insert(
                &mut map,
                STRING_KEY,
                (&descriptor as *const (*const u8, i64)).cast(),
                8,
            );
            *(slot as *mut i64) = 5;
            let other = key(&second);
            let found = zore_map_find(map, STRING_KEY, (&other as *const (*const u8, i64)).cast());
            assert_eq!(*(found as *const i64), 5);
            zore_map_free(map);
        }
    }

    #[test]
    fn keys_are_read_back_by_position_as_stored() {
        let _serial = crate::string::serial();
        let mut map: *mut Map = std::ptr::null_mut();
        for key in [5, 9] {
            insert(&mut map, key, key);
        }
        let text = b"key";
        let descriptor: (*const u8, i64) = (text.as_ptr(), 3);
        let mut names: *mut Map = std::ptr::null_mut();
        // SAFETY: both maps are live and positions are below their lengths.
        unsafe {
            assert_eq!(*(zore_map_key_at(map, 1) as *const i64), 9);
            zore_map_insert(
                &mut names,
                STRING_KEY,
                (&descriptor as *const (*const u8, i64)).cast(),
                0,
            );
            let stored = *(zore_map_key_at(names, 0) as *const (*const u8, i64));
            assert_eq!(stored, descriptor);
            zore_map_free(map);
            zore_map_free(names);
        }
    }

    #[test]
    fn text_keys_keep_their_text_until_the_entry_or_the_map_goes() {
        let _serial = crate::string::serial();
        use super::super::string::StringOut;
        let mut map: *mut Map = std::ptr::null_mut();
        let keys: Vec<_> = ["one", "two", "three"]
            .iter()
            .map(|text| super::super::string::StringOut::built(text.as_bytes()))
            .collect();
        // SAFETY: `map` is null or live, keys are 16-byte descriptors, values are 8 bytes.
        unsafe {
            for key in &keys {
                let slot =
                    zore_map_insert(&mut map, STRING_KEY, (key as *const StringOut).cast(), 8);
                *(slot as *mut i64) = 1;
            }
            for key in &keys {
                super::super::string::release(key.data, key.len as usize);
            }
            assert_eq!(super::super::string::live_buffers(), 3);
            let cloned = zore_map_clone_shape(map);
            let mut out = 0i64;
            assert!(zore_map_detach(
                map,
                STRING_KEY,
                (&keys[1] as *const StringOut).cast(),
                (&mut out as *mut i64).cast()
            ));
            assert_eq!(super::super::string::live_buffers(), 3);
            zore_map_free(map);
            assert_eq!(super::super::string::live_buffers(), 3);
            zore_map_free(cloned);
        }
        assert_eq!(super::super::string::live_buffers(), 0);
    }
}
