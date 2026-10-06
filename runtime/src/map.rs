//! Keys are `kind` raw bytes or a string matched by content; each value has a stable address.

use std::collections::HashMap;

use super::alloc::{zore_alloc, zore_free};

const STRING_KEY: i32 = 16;

pub struct Map {
    index: HashMap<Vec<u8>, usize>,
    entries: Vec<(Vec<u8>, *mut u8)>,
    value_size: i64,
}

/// The canonical bytes of the key at `key`.
///
/// # Safety
/// `key` must point to a live key of the layout `kind` names.
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
        .map_or(std::ptr::null_mut(), |&index| map.entries[index].1)
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
            }));
        }
        &mut **slot
    };
    let key = unsafe { key_bytes(kind, key) };
    let value = zore_alloc(value_size);
    map.index.insert(key.clone(), map.entries.len());
    map.entries.push((key, value));
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
    let (_, value) = map.entries.swap_remove(index);
    if let Some((moved_key, _)) = map.entries.get(index) {
        map.index.insert(moved_key.clone(), index);
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
    map.entries[usize::try_from(index).unwrap_or(usize::MAX)].1
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
    for (_, value) in map.entries {
        // SAFETY: each value came from `zore_alloc(map.value_size)`.
        unsafe { zore_free(value, map.value_size) };
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
    fn string_keys_match_by_content() {
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
}
