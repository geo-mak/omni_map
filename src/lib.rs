mod vec;

use crate::vec::AllocVec;
use std::collections::VecDeque;
use std::fmt;
use std::hash::{DefaultHasher, Hash, Hasher};

const LOAD_FACTOR: f64 = 0.75; // 75% threshold for reallocation
const DEFAULT_CAPACITY: usize = 16; // Default capacity of the map

/// A key-value map that maintains insertion order.
#[derive(Debug, Clone, PartialEq)]
pub struct OmniMap<K, V> {
    entries: AllocVec<(K, V)>,
    indices: AllocVec<VecDeque<usize>>,
}

impl<K, V> OmniMap<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    /// Creates a new `OmniMap` with `0` initial capacity.
    ///
    /// # Examples
    ///
    /// ```
    /// use omni_map::OmniMap;
    ///
    /// let map: OmniMap<String, i32> = OmniMap::new();
    ///
    /// assert_eq!(map.len(), 0);
    /// assert_eq!(map.capacity(), 0);
    /// ```
    ///
    #[inline]
    pub fn new() -> Self {
        OmniMap {
            entries: AllocVec::new(),
            indices: AllocVec::new(),
        }
    }

    /// Creates a new `OmniMap` with the specified capacity.
    ///
    /// # Parameters
    /// - `capacity`: The initial capacity of the map.
    ///
    /// # Examples
    ///
    /// ```
    /// use omni_map::OmniMap;
    ///
    /// let map: OmniMap<String, i32> = OmniMap::with_capacity(10);
    ///
    /// assert_eq!(map.len(), 0);
    /// assert_eq!(map.capacity(), 10);
    /// ```
    ///
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        OmniMap {
            entries: AllocVec::with_capacity(capacity),
            indices: AllocVec::with_capacity_and_populate(capacity),
        }
    }

    /// Calculates the hash value for a key.
    fn hash(&self, key: &K) -> usize {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        hasher.finish() as usize
    }

    /// Computes the index where `index = h(a) % indices.len()`.
    fn compute_index(&self, key: &K) -> usize {
        self.hash(key) % self.indices.len()
    }

    /// Updates the `indices` after a resizing.
    /// This involves rehashing the keys
    fn update_indices(&mut self) {
        for i in 0..self.len() {
            let (key, _) = &self.entries[i];
            let new_index = self.hash(key) % self.indices.len();
            self.indices[new_index].push_back(i);
        }
    }

    /// Returns the number of key-value pairs in the `OmniMap`.
    ///
    /// # Examples
    ///
    /// ```
    /// use omni_map::OmniMap;
    ///
    /// let mut map = OmniMap::new();
    ///
    /// assert_eq!(map.len(), 0);
    ///
    /// map.upsert("key1".to_string(), "value1".to_string());
    /// map.upsert("key2".to_string(), "value2".to_string());
    ///
    /// assert_eq!(map.len(), 2);
    /// ```
    ///
    #[inline]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Checks if the `OmniMap` is empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use omni_map::OmniMap;
    ///
    /// let map: OmniMap<String, String> = OmniMap::new();
    ///
    /// assert!(map.is_empty());
    ///
    /// let mut map = OmniMap::new();
    ///
    /// map.upsert("key1".to_string(), "value1".to_string());
    ///
    /// assert!(!map.is_empty());
    /// ```
    ///
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.entries.len() == 0
    }

    /// Returns the capacity of the `OmniMap`.
    ///
    /// # Examples
    ///
    /// ```
    /// use omni_map::OmniMap;
    ///
    /// let map: OmniMap<String, i32> = OmniMap::new();
    /// assert_eq!(map.capacity(), 0);
    ///
    /// let map: OmniMap<String, i32> = OmniMap::default();
    /// assert_eq!(map.capacity(), 16);
    ///
    /// let map: OmniMap<String, i32> = OmniMap::with_capacity(10);
    /// assert_eq!(map.capacity(), 10);
    /// ```
    ///
    #[inline]
    pub fn capacity(&self) -> usize {
        self.entries.capacity()
    }

    /// Checks if the map needs more capacity.
    fn needs_capacity(&self) -> bool {
        // Load factor = number of entries / capacity (the actual capacity of the entries vector)
        let load_factor = self.entries.len() as f64 / self.entries.capacity() as f64;
        load_factor > LOAD_FACTOR
    }

    /// Expands the capacity of the map.
    /// It **does not** rehash the entries and update the indices.
    ///
    /// # Parameters
    ///
    /// - `additional`: The number of additional slots to allocate.
    ///
    fn grow_by(&mut self, additional: usize) {
        // This is ensured by the calling contexts.
        debug_assert!(additional > 0);

        let new_len = self.entries.capacity() + additional;

        // Resize indices vector with new cells
        self.indices.resize_with(new_len, VecDeque::new);

        // Only reserve the additional capacity
        self.entries.reserve(additional);

        // Entries and indices vectors must maintain the same capacity
        debug_assert_eq!(self.entries.capacity(), self.indices.capacity());
    }

    /// Resizes the map if necessary and rehashes `entries`.
    fn maybe_grow(&mut self) {
        if self.entries.capacity() == 0 {
            self.grow_by(1);
        } else if self.needs_capacity() {
            // Calculate additional capacity
            let additional: usize = {
                let growth_factor = (self.entries.capacity() as f64 / LOAD_FACTOR).ceil() as usize;

                let growth_factor = growth_factor
                    .checked_next_power_of_two()
                    .unwrap_or(usize::MAX);

                growth_factor - self.entries.capacity()
            };

            // Allocate the additional capacity
            self.grow_by(additional);

            // Rehash the entries into the new resized indices
            self.update_indices();
        }
    }

    /// Reserves capacity for `additional` more elements.
    /// The resulting capacity will be equal to `self.capacity() + additional` exactly.
    ///
    /// # Time Complexity
    /// - *O*(n) in the worst case due to resizing.
    ///
    /// # Parameters
    /// - `additional`: The number of additional elements to reserve space for.
    ///
    /// # Examples
    ///
    /// ```
    /// use omni_map::OmniMap;
    ///
    /// let mut map = OmniMap::new();
    /// map.upsert("key1".to_string(), 1);
    ///
    /// // Reserve space for 1000 more elements
    /// map.reserve(1000);
    ///
    /// assert!(map.capacity() >= 1001);
    /// ```
    ///
    pub fn reserve(&mut self, additional: usize) {
        if additional == 0 {
            return;
        }
        self.grow_by(additional);
        // Rehash the entries into the new resized indices
        self.update_indices();
    }

    /// Inserts or updates a value with a given key.
    ///
    /// If the key already exists, its value is updated. If the key does not exist, a new entry is
    /// added.
    ///
    /// # Parameters
    /// - `key`: The key to insert or update.
    /// - `value`: The value to associate with the key.
    ///
    /// # Time Complexity
    /// - Amortized *O*(1).
    /// - *O*(n) in the worst case due to resizing.
    ///
    /// # Examples
    /// ```
    /// use omni_map::OmniMap;
    ///
    /// let mut map = OmniMap::new();
    ///
    /// map.upsert("key1".to_string(), "value1".to_string());
    /// map.upsert("key2".to_string(), "value2".to_string());
    ///
    /// assert_eq!(map.get(&"key1".to_string()), Some(&"value1".to_string()));
    /// assert_eq!(map.get(&"key2".to_string()), Some(&"value2".to_string()));
    /// ```
    ///
    pub fn upsert(&mut self, key: K, value: V) {
        // Resize if necessary
        self.maybe_grow();

        // Compute the index for the key
        let cmp_index = self.compute_index(&key);

        // Get the index vector for the computed index
        let index = &mut self.indices[cmp_index];

        // Check if the key already exists
        if let Some(entry_index) = index.iter().find(|&&i| self.entries[i].0 == key) {
            // Update the existing entry
            let entry = &mut self.entries[*entry_index];
            entry.1 = value;
        } else {
            // Push the new entry
            let new_index = self.entries.len();
            self.entries.push((key, value));
            self.indices[cmp_index].push_back(new_index);
        }
    }

    /// Retrieves a value by its key.
    ///
    /// # Parameters
    /// - `key`: The key for which to retrieve the value.
    ///
    /// # Returns
    /// An `Option` containing the value if the key is found, or `None` if the key does not exist.
    ///
    /// # Time Complexity
    /// - *O*(1) on average.
    /// - *O*(n) in the worst case, where `n` is the number of collided keys.
    ///
    /// # Examples
    /// ```
    /// use omni_map::OmniMap;
    ///
    /// let mut map = OmniMap::new();
    ///
    /// map.upsert("key1".to_string(), "value1".to_string());
    ///
    /// assert_eq!(map.get(&"key1".to_string()), Some(&"value1".to_string()));
    /// assert_eq!(map.get(&"nonexistent_key".to_string()), None);
    /// ```
    ///
    #[inline]
    pub fn get(&self, key: &K) -> Option<&V> {
        // Compute the index for the key
        let cmp_index = self.compute_index(key);

        // Get the index vector for the computed index
        self.indices[cmp_index].iter().find_map(|&i| {
            // Return the value if the key is found
            if &self.entries[i].0 == key {
                Some(&self.entries[i].1)
            } else {
                None
            }
        })
    }

    /// Retrieves a mutable reference to a value by its key.
    ///
    /// # Parameters
    /// - `key`: The key for which to retrieve the mutable reference to the value.
    ///
    /// # Returns
    /// An `Option` containing a mutable reference to the value if the key is found, or `None` if the key does not exist.
    ///
    /// # Time Complexity
    /// - *O*(1) on average.
    /// - *O*(n) in the worst case, where `n` is the number of collided keys.
    ///
    /// # Examples
    /// ```
    /// use omni_map::OmniMap;
    ///
    /// let mut map = OmniMap::new();
    ///
    /// map.upsert("key1".to_string(), "value1".to_string());
    ///
    /// if let Some(value) = map.get_mut(&"key1".to_string()) {
    ///     *value = "new_value1".to_string();
    /// }
    ///
    /// assert_eq!(map.get(&"key1".to_string()), Some(&"new_value1".to_string()));
    /// assert_eq!(map.get_mut(&"nonexistent_key".to_string()), None);
    /// ```
    ///
    #[inline]
    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        // Compute the index for the key
        let cmp_index = self.compute_index(key);

        // Get the index vector for the computed index
        for &i in &self.indices[cmp_index] {
            if &self.entries[i].0 == key {
                return Some(&mut self.entries[i].1);
            }
        }
        None
    }

    /// Returns the first entry in the map.
    ///
    /// # Returns
    /// An `Option` containing the key-value pair of the first inserted entry if the map is not empty.
    ///
    /// # Time Complexity
    /// - *O*(1).
    ///
    /// # Examples
    /// ```
    /// use omni_map::OmniMap;
    ///
    /// let mut map = OmniMap::new();
    /// map.upsert("key1".to_string(), 1);
    /// map.upsert("key2".to_string(), 2);
    /// map.upsert("key3".to_string(), 3);
    ///
    /// assert_eq!(map.first(), Some((&"key1".to_string(), &1)));
    /// ```
    ///
    #[inline]
    pub fn first(&self) -> Option<(&K, &V)> {
        self.entries.first().map(|(key, value)| (key, value))
    }

    /// Returns the last entry in the map.
    ///
    /// # Returns
    /// An `Option` containing the key-value pair of the last inserted entry if the map is not empty.
    ///
    /// # Time Complexity
    /// - *O*(1).
    ///
    /// # Examples
    /// ```
    /// use omni_map::OmniMap;
    ///
    /// let mut map = OmniMap::new();
    /// map.upsert("key1".to_string(), 1);
    /// map.upsert("key2".to_string(), 2);
    /// map.upsert("key3".to_string(), 3);
    ///
    /// assert_eq!(map.last(), Some((&"key3".to_string(), &3)));
    /// ```
    ///
    #[inline]
    pub fn last(&self) -> Option<(&K, &V)> {
        self.entries.last().map(|(key, value)| (key, value))
    }

    /// Decrements all indices greater than the given index
    fn decrement_indices(&mut self, after: usize) {
        for index in &mut self.indices {
            for i in index.iter_mut() {
                if *i > after {
                    *i -= 1;
                }
            }
        }
    }

    /// Removes an entry by its key.
    ///
    /// # Parameters
    /// - `key`: The key to remove.
    ///
    /// # Returns
    /// `true` if the key was found and removed, `false` otherwise.
    ///
    /// # Time Complexity
    /// - *O*(n) in the worst case.
    /// - *O*(1) if the entry is the last entry.
    ///
    /// # Note
    /// This method does not shrink the current capacity of the map.
    ///
    /// # Examples
    /// ```
    /// use omni_map::OmniMap;
    ///
    /// let mut map = OmniMap::new();
    ///
    /// map.upsert("key1".to_string(), "value1".to_string());
    /// map.upsert("key2".to_string(), "value2".to_string());
    ///
    /// assert_eq!(map.len(), 2);
    ///
    /// assert!(map.remove(&"key1".to_string()));
    ///
    /// assert_eq!(map.len(), 1);
    ///
    /// assert!(!map.remove(&"nonexistent_key".to_string()));
    /// ```
    ///
    pub fn remove(&mut self, key: &K) -> bool {
        if self.is_empty() {
            return false;
        }

        // Compute the index for the key
        let cmp_index = self.compute_index(key);
        let index = &mut self.indices[cmp_index];

        // Find the index of the key in the entries
        if let Some(entry_index) = index.iter().position(|&i| &self.entries[i].0 == key) {
            // Remove the entry from the index
            let entry_index = index.remove(entry_index).unwrap();

            let last_index = self.entries.len() - 1;

            // Pop the entry if it is the last entry
            if entry_index == last_index {
                self.entries.pop();
            } else {
                // Remove the entry from the entries
                self.entries.remove(entry_index);
                self.decrement_indices(entry_index);
            }

            true
        } else {
            false
        }
    }

    /// Pops the first entry from the map.
    ///
    /// # Returns
    /// An `Option` containing the key-value pair if the map is not empty.
    ///
    /// # Time Complexity
    /// - *O*(n) in the worst case.
    ///
    /// # Note
    /// This method does not shrink the current capacity of the map.
    ///
    /// # Examples
    /// ```
    /// use omni_map::OmniMap;
    ///
    /// let mut map = OmniMap::new();
    /// map.upsert("key1".to_string(), 1);
    /// map.upsert("key2".to_string(), 2);
    /// map.upsert("key3".to_string(), 3);
    ///
    /// assert_eq!(map.pop_front(), Some(("key1".to_string(), 1)));
    /// assert_eq!(map.len(), 2);
    /// ```
    pub fn pop_front(&mut self) -> Option<(K, V)> {
        if self.is_empty() {
            return None;
        }

        // Pop the first entry
        if let Some((key, value)) = self.entries.pop_front() {
            let index = &mut self.indices[0];

            // Remove the entry index from the corresponding index in indices vector
            if let Some(pos) = index.iter().position(|&i| i == 0) {
                index.remove(pos);
            }
            // Decrement all indices greater than 0
            self.decrement_indices(0);
            Some((key, value))
        } else {
            None
        }
    }

    /// Pops the last entry from the map.
    ///
    /// # Returns
    /// An `Option` containing the key-value pair if the map is not empty.
    ///
    /// # Time Complexity
    /// - *O*(1) on average.
    ///
    /// # Note
    /// This method does not shrink the current capacity of the map.
    ///
    /// # Examples
    /// ```
    /// use omni_map::OmniMap;
    ///
    /// let mut map = OmniMap::new();
    /// map.upsert("key1".to_string(), 1);
    /// map.upsert("key2".to_string(), 2);
    /// map.upsert("key3".to_string(), 3);
    ///
    /// assert_eq!(map.pop(), Some(("key3".to_string(), 3)));
    /// assert_eq!(map.len(), 2);
    /// ```
    ///
    #[inline]
    pub fn pop(&mut self) -> Option<(K, V)> {
        if self.is_empty() {
            return None;
        }

        // Pop the last entry
        let last_index = self.entries.len() - 1;
        let (key, value) = self.entries.pop().unwrap();

        // Remove the last index from the corresponding index vector
        let cmp_index = self.compute_index(&key);
        let index = &mut self.indices[cmp_index];

        // Remove the entry index from the corresponding index in indices vector
        if let Some(pos) = index.iter().position(|&i| i == last_index) {
            index.remove(pos);
        }
        Some((key, value))
    }

    /// Clears the map, removing all key-value pairs.
    ///
    /// # Note
    /// This method does not shrink the current capacity of the map.
    ///
    /// # Time Complexity
    /// - *O*(n).
    ///
    /// # Examples
    /// ```
    /// use omni_map::OmniMap;
    ///
    /// let mut map = OmniMap::new();
    /// map.upsert("key1".to_string(), "value1".to_string());
    /// map.upsert("key2".to_string(), "value2".to_string());
    ///
    /// assert_eq!(map.len(), 2);
    ///
    /// map.clear();
    ///
    /// assert_eq!(map.len(), 0);
    /// ```
    #[inline]
    pub fn clear(&mut self) {
        self.indices.clear();
        self.entries.clear();
    }

    /// Returns an iterator over the key-value pairs in the `OmniMap`.
    ///
    /// # Examples
    ///
    /// ```
    /// use omni_map::OmniMap;
    ///
    /// let mut map = OmniMap::new();
    ///
    /// map.upsert("key1".to_string(), "value1".to_string());
    /// map.upsert("key2".to_string(), "value2".to_string());
    ///
    /// let entries: Vec<(String, String)> = map.iter().map(|(key, value)| (key.clone(), value.clone())).collect();
    ///
    /// assert_eq!(entries, vec![("key1".to_string(), "value1".to_string()),
    ///                           ("key2".to_string(), "value2".to_string())]);
    /// ```
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.entries.iter().map(|(key, value)| (key, value))
    }

    /// Returns an iterator over the keys in the `OmniMap`.
    ///
    /// # Examples
    ///
    /// ```
    /// use omni_map::OmniMap;
    ///
    /// let mut map = OmniMap::new();
    ///
    /// map.upsert("key1".to_string(), "value1".to_string());
    /// map.upsert("key2".to_string(), "value2".to_string());
    ///
    /// let keys: Vec<String> = map.iter_keys().cloned().collect();
    /// assert_eq!(keys, vec!["key1", "key2"]);
    /// ```
    #[inline]
    pub fn iter_keys(&self) -> impl Iterator<Item = &K> {
        self.entries.iter().map(|(key, _)| key)
    }

    /// Returns an iterator over the values in the `OmniMap`.
    ///
    /// # Examples
    ///
    /// ```
    /// use omni_map::OmniMap;
    ///
    /// let mut map = OmniMap::new();
    ///
    /// map.upsert("key1".to_string(), "value1".to_string());
    /// map.upsert("key2".to_string(), "value2".to_string());
    ///
    /// let values: Vec<String> = map.iter_values().cloned().collect();
    ///
    /// assert_eq!(values, vec!["value1", "value2"]);
    /// ```
    #[inline]
    pub fn iter_values(&self) -> impl Iterator<Item = &V> {
        self.entries.iter().map(|(_, value)| value)
    }

    /// Returns the total memory usage of the `OmniMap` in bytes.
    pub fn memory_usage(&self) -> usize {
        // Memory used by the entries vector, including the AllocVec overhead
        let entries_memory: usize = size_of::<AllocVec<(K, V)>>()
            + self
                .entries
                .iter()
                .map(|(key, value)| size_of_val(key) + size_of_val(value))
                .sum::<usize>();

        // Memory used by the indices vector, including the AllocVec and VecDeque overhead
        let indices_memory: usize = size_of::<AllocVec<VecDeque<usize>>>()
            + self
                .indices
                .iter()
                .map(|vec_deque| {
                    size_of::<VecDeque<usize>>() + vec_deque.capacity() * size_of::<usize>()
                })
                .sum::<usize>();

        // Total memory usage
        entries_memory + indices_memory
    }
}

impl<K, V> fmt::Display for OmniMap<K, V>
where
    K: fmt::Debug,
    V: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{{")?;
        let mut first = true;
        for (key, value) in &self.entries {
            if !first {
                write!(f, ", ")?;
            }
            write!(f, "{:?}: {:?}", key, value)?;
            first = false;
        }
        write!(f, "}}")
    }
}

impl<K, V> Default for OmniMap<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    /// Creates a new `OmniMap` with the default capacity.
    /// The default capacity is set to `16`.
    ///
    /// # Examples
    /// ```
    /// use omni_map::OmniMap;
    ///
    /// let map: OmniMap<String, i32> = OmniMap::default();
    ///
    /// assert_eq!(map.capacity(), 16);
    /// ```
    ///
    #[inline]
    fn default() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }
}

impl<'a, K, V> IntoIterator for &'a OmniMap<K, V> {
    type Item = (&'a K, &'a V);
    type IntoIter = std::iter::Map<std::slice::Iter<'a, (K, V)>, fn(&(K, V)) -> (&K, &V)>;

    /// Returns an iterator over the key-value pairs in the `OmniMap`.
    fn into_iter(self) -> Self::IntoIter {
        self.entries.iter().map(|(key, value)| (key, value))
    }
}

impl<'a, K, V> IntoIterator for &'a mut OmniMap<K, V> {
    type Item = (&'a K, &'a mut V);
    type IntoIter =
        std::iter::Map<std::slice::IterMut<'a, (K, V)>, fn(&mut (K, V)) -> (&K, &mut V)>;

    /// Returns a mutable iterator over the key-value pairs in the `OmniMap`.
    fn into_iter(self) -> Self::IntoIter {
        self.entries.iter_mut().map(|(key, value)| (key, value))
    }
}

pub struct OmniMapIntoIter<K, V> {
    map: OmniMap<K, V>,
    index: usize,
}

impl<K, V> Iterator for OmniMapIntoIter<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    type Item = (K, V);

    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.map.len() {
            let item = self.map.entries.remove(self.index);
            Some(item)
        } else {
            None
        }
    }
}

impl<K, V> IntoIterator for OmniMap<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    type Item = (K, V);
    type IntoIter = OmniMapIntoIter<K, V>;

    /// Consumes the `OmniMap` and returns an iterator over its key-value pairs.
    ///
    /// # Returns
    /// An iterator that yields key-value pairs in the order they were inserted.
    ///
    /// # Examples
    ///
    /// ```
    /// use omni_map::OmniMap;
    ///
    /// let mut map = OmniMap::new();
    /// map.upsert("key1".to_string(), 1);
    /// map.upsert("key2".to_string(), 2);
    ///
    /// let mut iter = map.into_iter();
    /// assert_eq!(iter.next(), Some(("key1".to_string(), 1)));
    /// assert_eq!(iter.next(), Some(("key2".to_string(), 2)));
    /// assert_eq!(iter.next(), None);
    /// ```
    ///
    fn into_iter(self) -> Self::IntoIter {
        OmniMapIntoIter {
            map: self,
            index: 0,
        }
    }
}

#[cfg(test)]
mod tests {

    use crate::OmniMap;

    #[test]
    fn test_create_map() {
        let map: OmniMap<String, i32> = OmniMap::new();
        assert!(map.is_empty());
        assert_eq!(map.len(), 0);
        assert_eq!(map.capacity(), 0);
    }

    #[test]
    fn test_create_map_with_capacity() {
        let map: OmniMap<String, i32> = OmniMap::with_capacity(1000);
        assert!(map.is_empty());
        assert_eq!(map.len(), 0);
        assert_eq!(map.capacity(), 1000);
    }

    #[test]
    fn test_create_map_default() {
        let map: OmniMap<String, i32> = OmniMap::default();
        assert!(map.is_empty());
        assert_eq!(map.len(), 0);
        assert_eq!(map.capacity(), 16);
    }

    #[test]
    fn test_map_get() {
        let mut map = OmniMap::new();

        map.upsert("key1".to_string(), 1);
        map.upsert("key2".to_string(), 2);

        assert!(!map.is_empty());
        assert_eq!(map.len(), 2);

        assert_eq!(map.get(&"key1".to_string()), Some(&1));
        assert_eq!(map.get(&"key2".to_string()), Some(&2));
        assert_eq!(map.get(&"nonexistent".to_string()), None);
    }

    #[test]
    fn test_map_get_mut() {
        let mut map = OmniMap::new();

        map.upsert("key1".to_string(), 1);

        if let Some(value) = map.get_mut(&"key1".to_string()) {
            *value = 10;
        }

        assert_eq!(map.get(&"key1".to_string()), Some(&10));
    }

    #[test]
    fn test_map_update() {
        let mut map = OmniMap::new();
        map.upsert("key1".to_string(), 1);
        map.upsert("key1".to_string(), 2);

        assert_eq!(map.len(), 1);

        assert_eq!(map.get(&"key1".to_string()), Some(&2));
    }

    #[test]
    fn test_map_insertion_order() {
        let mut map = OmniMap::new();
        map.upsert("key1".to_string(), 1);
        map.upsert("key2".to_string(), 2);
        map.upsert("key3".to_string(), 3);

        // Check the order of the items
        assert_eq!(
            map.iter().collect::<Vec<(&String, &i32)>>(),
            vec![
                (&"key1".to_string(), &1),
                (&"key2".to_string(), &2),
                (&"key3".to_string(), &3)
            ]
        );
    }

    #[test]
    fn test_map_get_first() {
        let mut map = OmniMap::new();
        map.upsert("key1".to_string(), 1);
        map.upsert("key2".to_string(), 2);
        map.upsert("key3".to_string(), 3);

        assert_eq!(map.first(), Some((&"key1".to_string(), &1)));
    }

    #[test]
    fn test_map_get_last() {
        let mut map = OmniMap::new();
        map.upsert("key1".to_string(), 1);
        map.upsert("key2".to_string(), 2);
        map.upsert("key3".to_string(), 3);

        assert_eq!(map.last(), Some((&"key3".to_string(), &3)));
    }

    #[test]
    fn test_map_pop_front() {
        let mut map = OmniMap::new();
        map.upsert("key1".to_string(), 1);
        map.upsert("key2".to_string(), 2);
        map.upsert("key3".to_string(), 3);

        assert_eq!(map.len(), 3);

        let removed_item = map.pop_front();
        assert_eq!(removed_item, Some(("key1".to_string(), 1)));

        assert_eq!(map.len(), 2);
        assert_eq!(map.get(&"key1".to_string()), None);

        // First key now must be the second key
        assert_eq!(map.first(), Some((&"key2".to_string(), &2)));
    }

    #[test]
    fn test_map_pop() {
        let mut map = OmniMap::new();
        map.upsert("key1".to_string(), 1);
        map.upsert("key2".to_string(), 2);
        map.upsert("key3".to_string(), 3);

        assert_eq!(map.len(), 3);

        let removed_item = map.pop();

        assert_eq!(removed_item, Some(("key3".to_string(), 3)));

        assert_eq!(map.len(), 2);

        assert_eq!(map.get(&"key3".to_string()), None);

        // Last key now must be the second key
        assert_eq!(map.last(), Some((&"key2".to_string(), &2)));
    }

    #[test]
    fn test_map_delete_existing_key() {
        let mut map = OmniMap::new();
        map.upsert("key1".to_string(), 1);
        map.upsert("key2".to_string(), 2);

        assert_eq!(map.len(), 2);

        assert!(map.remove(&"key2".to_string()));

        assert_eq!(map.len(), 1);

        assert_eq!(map.get(&"key2".to_string()), None);
        assert_eq!(map.get(&"key1".to_string()), Some(&1));
    }

    #[test]
    fn test_map_delete_nonexistent_key() {
        let mut map = OmniMap::new();
        map.upsert("key1".to_string(), 1);

        assert_eq!(map.len(), 1);

        // Must return false, because the key does not exist
        assert!(!map.remove(&"nonexistent_key".to_string()));

        assert_eq!(map.len(), 1);

        assert_eq!(map.get(&"key1".to_string()), Some(&1));
    }

    #[test]
    fn test_map_delete_and_preserve_order() {
        let mut map = OmniMap::new();

        // Insert 4 items
        map.upsert("key1".to_string(), 1);
        map.upsert("key2".to_string(), 2);
        map.upsert("key3".to_string(), 3);
        map.upsert("key4".to_string(), 4);

        assert_eq!(map.len(), 4);

        // Remove the second item ("key2")
        assert!(map.remove(&"key2".to_string()));

        assert_eq!(map.len(), 3);

        // Check the order of the remaining items
        assert_eq!(
            map.iter().collect::<Vec<(&String, &i32)>>(),
            vec![
                (&"key1".to_string(), &1),
                (&"key3".to_string(), &3),
                (&"key4".to_string(), &4)
            ]
        );

        // Order of the keys should be preserved, but index has been updated
        // Test access by key
        assert_eq!(map.get(&"key3".to_string()), Some(&3));
    }

    #[test]
    fn test_map_load_and_allocation() {
        let mut map = OmniMap::with_capacity(4);

        // Insert 4 items
        map.upsert("key1".to_string(), 1);
        map.upsert("key2".to_string(), 2);
        map.upsert("key3".to_string(), 3);
        map.upsert("key4".to_string(), 4);

        // Capacity of the map before reallocation
        assert_eq!(map.capacity(), 4);

        // Insert one more item
        map.upsert("key5".to_string(), 5);

        // Capacity of the map after reallocation
        assert_eq!(map.capacity(), 8);
    }

    #[test]
    fn test_map_reserve_capacity() {
        let mut map = OmniMap::new();

        assert_eq!(map.capacity(), 0);

        map.upsert("key1".to_string(), 1);

        assert_eq!(map.capacity(), 1);

        // Reserve capacity in advance
        map.reserve(1000);

        // Capacity must be 1001
        assert_eq!(map.capacity(), 1001);

        assert_eq!(map.get(&"key1".to_string()), Some(&1));
    }

    #[test]
    fn test_into_iter_ordered() {
        let mut map = OmniMap::new();
        map.upsert("key1".to_string(), 1);
        map.upsert("key2".to_string(), 2);
        map.upsert("key3".to_string(), 3);

        let mut iter = map.into_iter();

        assert_eq!(iter.next(), Some(("key1".to_string(), 1)));
        assert_eq!(iter.next(), Some(("key2".to_string(), 2)));
        assert_eq!(iter.next(), Some(("key3".to_string(), 3)));
        assert_eq!(iter.next(), None);
    }
}
