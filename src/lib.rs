use std::collections::BTreeMap;

/// An in-memory key-value store backed by a `BTreeMap` for ordered iteration.
#[derive(Debug, Default)]
pub struct Store {
    data: BTreeMap<Vec<u8>, Vec<u8>>,
}

impl Store {
    pub fn new() -> Self {
        Self {
            data: BTreeMap::new(),
        }
    }

    /// Insert a key-value pair. Returns the previous value if the key already existed.
    pub fn put(&mut self, key: impl Into<Vec<u8>>, value: impl Into<Vec<u8>>) -> Option<Vec<u8>> {
        self.data.insert(key.into(), value.into())
    }

    /// Retrieve the value associated with `key`.
    pub fn get(&self, key: &[u8]) -> Option<&[u8]> {
        self.data.get(key).map(|v| v.as_slice())
    }

    /// Remove a key and return its value.
    pub fn delete(&mut self, key: &[u8]) -> Option<Vec<u8>> {
        self.data.remove(key)
    }

    /// Return the number of entries in the store.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Return `true` when the store contains no entries.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_and_get() {
        let mut store = Store::new();
        assert!(store.put(b"key1".to_vec(), b"value1".to_vec()).is_none());
        assert_eq!(store.get(b"key1"), Some(b"value1".as_slice()));
    }

    #[test]
    fn put_overwrite_returns_previous() {
        let mut store = Store::new();
        store.put(b"k".to_vec(), b"v1".to_vec());
        let prev = store.put(b"k".to_vec(), b"v2".to_vec());
        assert_eq!(prev.as_deref(), Some(b"v1".as_slice()));
        assert_eq!(store.get(b"k"), Some(b"v2".as_slice()));
    }

    #[test]
    fn get_missing_key_returns_none() {
        let store = Store::new();
        assert_eq!(store.get(b"nonexistent"), None);
    }

    #[test]
    fn delete_existing_key() {
        let mut store = Store::new();
        store.put(b"del".to_vec(), b"me".to_vec());
        let removed = store.delete(b"del");
        assert_eq!(removed.as_deref(), Some(b"me".as_slice()));
        assert!(store.get(b"del").is_none());
    }

    #[test]
    fn delete_missing_key_returns_none() {
        let mut store = Store::new();
        assert!(store.delete(b"nope").is_none());
    }

    #[test]
    fn len_and_is_empty() {
        let mut store = Store::new();
        assert!(store.is_empty());
        assert_eq!(store.len(), 0);
        store.put(b"a".to_vec(), b"1".to_vec());
        assert!(!store.is_empty());
        assert_eq!(store.len(), 1);
    }
}
