mod vec;

use std::fmt;
use std::fmt::{Debug, Display};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::ops::{Index, IndexMut};

use crate::vec::{AllocVec, AllocVecIntoIter};

#[derive(Debug)]
pub struct Entry<K, V> {
    key: K,
    value: V,
    hash: usize,
}

impl<K, V> Entry<K, V> {
    fn new(key: K, value: V, hash: usize) -> Self {
        Self { key, value, hash }
    }
}

impl<K: Eq, V: PartialEq> PartialEq for Entry<K, V> {
    fn eq(&self, other: &Self) -> bool {
        self.key.eq(&other.key) && self.value.eq(&other.value) && self.hash.eq(&other.hash)
    }
}

impl<K, V> Clone for Entry<K, V>
where
    K: Clone,
    V: Clone,
{
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            value: self.value.clone(),
            hash: self.hash, // usize is Copy
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Slot {
    Empty,
    Deleted,
    Occupied(usize),
}

// Required to call AllocVec::with_capacity_and_populate()
impl Default for Slot {
    fn default() -> Self {
        Self::Empty
    }
}

/// A hybrid data structure that combines the best of both hash maps and vectors.
pub struct OmniMap<K, V> {
    // AllocVec does not allow zero-sized types and will panic if used.
    // Both Entry and Slot are guaranteed not to be zero-sized.
    entries: AllocVec<Entry<K, V>>,
    index: AllocVec<Slot>,
}

// Core implementation
impl<K, V> OmniMap<K, V>
where
    K: Eq + Hash,
{
    const LOAD_FACTOR: f64 = 0.75; // 75% threshold for growing

    /// Creates a new `OmniMap` with `0` initial capacity.
    ///
    /// # Examples
    ///
    /// ```
    /// use omni_map::OmniMap;
    ///
    /// let map: OmniMap<i32, &str> = OmniMap::new();
    ///
    /// assert_eq!(map.len(), 0);
    /// assert_eq!(map.capacity(), 0);
    /// ```
    #[must_use]
    #[inline]
    pub fn new() -> Self {
        OmniMap {
            // Empty vectors with dangling pointers
            entries: AllocVec::new(),
            index: AllocVec::new(),
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
    /// let map: OmniMap<i32, &str> = OmniMap::with_capacity(10);
    ///
    /// assert_eq!(map.len(), 0);
    /// assert_eq!(map.capacity(), 10);
    /// ```
    #[must_use = "Creating new instances with default capacity involves allocating memory."]
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        OmniMap {
            // Initialize the entries and only reserve capacity
            entries: AllocVec::with_capacity(capacity),
            // Initialize the index with empty slots by calling T::default()
            index: AllocVec::with_capacity_and_populate(capacity),
        }
    }

    /// Returns the capacity of the `OmniMap`.
    ///
    /// # Examples
    ///
    /// ```
    /// use omni_map::OmniMap;
    ///
    /// let map: OmniMap<i32, &str> = OmniMap::new();
    /// assert_eq!(map.capacity(), 0);
    ///
    /// let map: OmniMap<i32, &str> = OmniMap::default();
    /// assert_eq!(map.capacity(), 16);
    ///
    /// let map: OmniMap<i32, &str> = OmniMap::with_capacity(10);
    /// assert_eq!(map.capacity(), 10);
    /// ```
    #[inline]
    pub fn capacity(&self) -> usize {
        self.index.capacity()
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
    /// map.insert(1, "a");
    /// map.insert(2, "b");
    ///
    /// assert_eq!(map.len(), 2);
    /// ```
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
    /// let map: OmniMap<i32, &str> = OmniMap::new();
    ///
    /// assert!(map.is_empty());
    ///
    /// let mut map = OmniMap::new();
    ///
    /// map.insert(1, "a");
    ///
    /// assert!(!map.is_empty());
    /// ```
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.entries.len() == 0
    }

    /// Calculates the hash value for a key.
    #[inline]
    fn hash(&self, key: &K) -> usize {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        hasher.finish() as usize
    }

    /// Finds the slot of the key in the index.
    /// Returns the `(slot, index)` of the entry.
    fn find_index(&self, hash: &usize, key: &K) -> Option<(usize, usize)> {
        let capacity = self.index.capacity();
        let mut slot = hash % capacity;
        let mut step = 0;
        // Edge case: if capacity is full and all slots are occupied, it will be an infinite loop
        while step < capacity {
            if matches!(self.index[slot], Slot::Empty) {
                // Slot is empty, key does not exist
                return None;
            } else {
                // Slot is either occupied or deleted
                if let Slot::Occupied(index) = self.index[slot] {
                    if self.entries[index].key == *key {
                        return Some((slot, index));
                    }
                }
                // Search further until finding a key match or encountering an empty slot
                slot = (slot + 1) % capacity;
                step += 1;
            }
        }
        None
    }

    /// Rebuilds the index of the map.
    fn reindex(&mut self, capacity: &usize) {
        // This is ensured by the calling contexts.
        debug_assert!(self.entries.len() <= *capacity);
        let mut new_index: AllocVec<Slot> = AllocVec::with_capacity_and_populate(*capacity);
        for (index, entry) in self.entries.iter().enumerate() {
            let mut slot = entry.hash % *capacity;
            // Probe until an empty slot is found
            while !matches!(new_index[slot], Slot::Empty) {
                slot = (slot + 1) % *capacity;
            }
            // Empty slot found, set index
            new_index[slot] = Slot::Occupied(index);
        }
        self.index = new_index;
    }

    #[inline]
    fn decrement_index(&mut self, after: usize) {
        for slot in self.index.iter_mut() {
            if let Slot::Occupied(index) = slot {
                if *index > after {
                    *index -= 1;
                }
            }
        }
    }

    /// Expands the capacity of the map.
    ///
    /// # Parameters
    ///
    /// - `additional`: The number of additional slots to allocate.
    /// - `reindex`: If `true`, the map will rebuild the index with the additional slots.
    fn grow(&mut self, additional: usize, reindex: bool) {
        // This is ensured by the calling contexts.
        debug_assert!(additional > 0);

        // Reserve the additional capacity
        self.entries.reserve(additional);
        let new_cap = self.entries.capacity();

        if reindex {
            // Reindex with higher capacity
            self.reindex(&new_cap);
        } else {
            // Expand the index with new empty slots
            self.index.resize_with(new_cap, || Slot::Empty);
        }

        // Entries and indices vectors must maintain the same capacity
        debug_assert_eq!(self.entries.capacity(), self.index.capacity());
    }

    /// Resizes map with reindexing if the current load exceeds the load factor.
    fn maybe_grow(&mut self) {
        // Load factor = number of entries / capacity (the actual capacity of the index)
        let current_load = self.entries.len() as f64 / self.index.capacity() as f64;
        if current_load > Self::LOAD_FACTOR {
            // Calculate additional capacity
            let additional: usize = {
                let growth_factor =
                    (self.index.capacity() as f64 / Self::LOAD_FACTOR).ceil() as usize;

                let growth_factor = growth_factor
                    .checked_next_power_of_two()
                    .unwrap_or(usize::MAX); // Handle overflow

                growth_factor - self.index.capacity()
            };
            // Allocate the additional capacity with reindexing
            self.grow(additional, true);
        }
    }

    /// Reserves capacity for `additional` more elements.
    /// The resulting capacity will be equal to `self.capacity() + additional` exactly.
    ///
    /// # Time Complexity
    /// - *O*(n) on average, where *n* is the number of elements in the map.
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
    /// map.insert(1, "a");
    ///
    /// // Reserve space for 10 more elements
    /// map.reserve(10);
    ///
    /// // The capacity is now 11
    /// assert_eq!(map.capacity(), 11);
    /// ```
    #[inline]
    pub fn reserve(&mut self, additional: usize) {
        // Guard against zero additional capacity
        if additional == 0 {
            return;
        }
        self.grow(additional, true);
    }

    /// Shrinks the capacity of the `OmniMap` to the specified capacity.
    /// In order to take effect, `capacity` must be less than the current capacity
    /// and greater than or equal to the number of elements in the map.
    ///
    /// # Parameters
    /// - `capacity`: The new capacity of the map.
    ///
    /// # Note
    /// This method will reindex the map after shrinking.
    ///
    /// # Time Complexity
    /// - *O*(n) on average, where *n* is the number of elements in the map.
    ///
    /// # Examples
    /// ```
    /// use omni_map::OmniMap;
    ///
    /// let mut map = OmniMap::with_capacity(10);
    ///
    /// assert_eq!(map.capacity(), 10);
    ///
    /// // Insert some elements
    /// map.insert(1, "a");
    /// map.insert(2, "b");
    ///
    /// // Shrink the capacity to 3
    /// map.shrink_to(5);
    ///
    /// assert_eq!(map.capacity(), 5);
    /// ```
    #[inline]
    pub fn shrink_to(&mut self, capacity: usize) {
        // Capacity must be less than the current capacity and greater than or equal to the number of elements
        if capacity < self.index.capacity() && capacity >= self.entries.len() {
            self.entries.shrink_to(capacity);
            self.reindex(&self.entries.capacity());
        }
    }

    /// Shrinks the capacity of the `OmniMap` to fit its current length.
    /// If the capacity is equal to the number of elements in the map, this method will do nothing.
    ///
    /// # Note
    /// This method will reindex the map after shrinking.
    ///
    /// # Time Complexity
    /// - *O*(n) on average, where *n* is the number of elements in the map.
    ///
    /// # Examples
    /// ```
    /// use omni_map::OmniMap;
    ///
    /// let mut map = OmniMap::with_capacity(10);
    ///
    /// assert_eq!(map.capacity(), 10 );
    ///
    /// // Insert some elements
    ///  map.insert(1, "a");
    ///  map.insert(2, "b");
    ///
    /// // Shrink the capacity to fit the current length
    /// map.shrink_to_fit();
    ///
    /// assert_eq!(map.capacity(), 2);
    /// ```
    #[inline]
    pub fn shrink_to_fit(&mut self) {
        // Capacity must be greater to the number of elements
        if self.index.capacity() > self.entries.len() {
            self.entries.shrink_to_fit();
            self.reindex(&self.entries.capacity());
        }
    }

    /// Inserts a key-value pair into the map.
    /// If the map did not have this key present, `None` is returned.
    /// If the map did have this key present, the value is updated, and the old value is returned.
    ///
    /// # Parameters
    /// - `key`: The key to insert or update.
    /// - `value`: The value to associate with the key.
    ///
    /// # Time Complexity
    /// - Amortized *O*(1).
    ///
    /// # Examples
    /// ```
    /// use omni_map::OmniMap;
    ///
    /// let mut map = OmniMap::new();
    ///
    ///  // When inserting a new key-value pair, None is returned
    ///  map.insert(1, "a");
    ///  map.insert(2, "b");
    ///
    /// assert_eq!(map.get(&1), Some(&"a"));
    /// assert_eq!(map.get(&2), Some(&"b"));
    ///
    /// // Update the value for an existing key
    /// let old_value = map.insert(1, "c");
    ///
    /// // The old value is returned
    /// assert_eq!(old_value, Some("a"));
    ///
    /// // The value is updated
    /// assert_eq!(map.get(&1), Some(&"c"));
    /// ```
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        // Ensure that the map has enough capacity to insert the new key-value pair.
        if self.index.capacity() == 0 {
            // Allocate initial capacity without reindexing.
            self.grow(1, false);
        } else {
            // Resize if the current load exceeds the load factor.
            self.maybe_grow();
        }

        // Hash the key
        let hash = self.hash(&key);

        let capacity = self.index.capacity();

        // Calculate the slot.
        let mut slot = hash % capacity;

        // No infinite loop because maybe_grow() makes sure that capacity is larger than length.
        while !matches!(self.index[slot], Slot::Empty) {
            if let Slot::Occupied(index) = self.index[slot] {
                let entry = &mut self.entries[index];
                if entry.key == key {
                    // Key exists, update the value and return the old value.
                    let old_value = std::mem::replace(&mut entry.value, value);
                    return Some(old_value);
                }
                slot = (slot + 1) % capacity;
            } else {
                // Slot is deleted, reuse the slot.
                break;
            }
        }

        // Insert a new entry.
        self.entries.push(Entry::new(key, value, hash));
        let entry_index = self.entries.len() - 1;
        self.index[slot] = Slot::Occupied(entry_index);

        // Return None because the key did not exist.
        None
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
    ///
    /// # Examples
    /// ```
    /// use omni_map::OmniMap;
    ///
    /// let mut map = OmniMap::new();
    ///
    ///  map.insert(1, "a");
    ///
    /// assert_eq!(map.get(&1), Some(&"a"));
    ///
    /// // Key does not exist
    /// assert_eq!(map.get(&2), None);
    /// ```
    #[must_use = "Unused function call that returns without side effects"]
    #[inline]
    pub fn get(&self, key: &K) -> Option<&V> {
        let hash = self.hash(key);
        if let Some((_, index)) = self.find_index(&hash, key) {
            return Some(&self.entries[index].value);
        }
        None
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
    ///
    /// # Examples
    /// ```
    /// use omni_map::OmniMap;
    ///
    /// let mut map = OmniMap::new();
    ///
    /// map.insert(1, "a");
    ///
    /// if let Some(value) = map.get_mut(&1) {
    ///     *value = "b";
    /// }
    ///
    /// assert_eq!(map.get(&1), Some(&"b"));
    ///
    /// // Key does not exist
    /// assert_eq!(map.get_mut(&2), None);
    /// ```
    #[must_use = "Unused function call that returns without side effects"]
    #[inline]
    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        let hash = self.hash(key);
        if let Some((_, index)) = self.find_index(&hash, key) {
            return Some(&mut self.entries[index].value);
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
    /// map.insert(1, "a");
    /// map.insert(2, "b");
    /// map.insert(3, "c");
    ///
    /// assert_eq!(map.first(), Some((&1, &"a")));
    /// ```
    #[must_use = "Unused function call that returns without side effects"]
    #[inline]
    pub fn first(&self) -> Option<(&K, &V)> {
        if self.is_empty() {
            return None;
        }
        // This is safe because the map is not empty
        let entry = self.entries.first();
        Some((&entry.key, &entry.value))
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
    /// map.insert(1, "a");
    /// map.insert(2, "b");
    /// map.insert(3, "c");
    ///
    /// assert_eq!(map.last(), Some((&3, &"c")));
    /// ```
    #[must_use = "Unused function call that returns without side effects"]
    #[inline]
    pub fn last(&self) -> Option<(&K, &V)> {
        if self.is_empty() {
            return None;
        }
        // This is safe because the map is not empty
        let entry = self.entries.last();
        Some((&entry.key, &entry.value))
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
    /// map.insert(1, "a");
    /// map.insert(2, "b");
    ///
    /// assert_eq!(map.len(), 2);
    ///
    /// // Remove an existing key
    /// assert_eq!(map.remove(&1), Some("a"));
    ///
    /// assert_eq!(map.len(), 1);
    ///
    /// // Remove a non-existing key
    /// assert_eq!(map.remove(&1), None);
    /// ```
    pub fn remove(&mut self, key: &K) -> Option<V> {
        if self.is_empty() {
            return None;
        }
        let hash = self.hash(key);
        if let Some((slot, index)) = self.find_index(&hash, key) {
            let entry: Entry<K, V>;
            if index == self.entries.len() - 1 {
                // This is safe because the map is not empty
                entry = self.entries.pop();
                self.index[slot] = Slot::Deleted;
            } else {
                entry = self.entries.remove(index);
                self.index[slot] = Slot::Deleted;
                self.decrement_index(index);
            }
            return Some(entry.value);
        }
        None
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
    /// map.insert(1, "a");
    /// map.insert(2, "b");
    /// map.insert(3, "c");
    ///
    /// assert_eq!(map.pop_front(), Some((1, "a")));
    /// assert_eq!(map.len(), 2);
    /// ```
    #[inline]
    pub fn pop_front(&mut self) -> Option<(K, V)> {
        if self.is_empty() {
            return None;
        }
        // This is safe because the map is not empty
        let entry = self.entries.pop_front();
        self.decrement_index(0);
        Some((entry.key, entry.value))
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
    /// map.insert(1, "a");
    /// map.insert(2, "b");
    /// map.insert(3, "c");
    ///
    /// assert_eq!(map.pop(), Some((3, "c")));
    /// assert_eq!(map.len(), 2);
    /// ```
    #[inline]
    pub fn pop(&mut self) -> Option<(K, V)> {
        if self.is_empty() {
            return None;
        }
        let entry = self.entries.last();
        if let Some((slot, _)) = self.find_index(&entry.hash, &entry.key) {
            self.index[slot] = Slot::Deleted;
            // This is safe because the map is not empty
            let entry = self.entries.pop();
            return Some((entry.key, entry.value));
        }
        None
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
    /// map.insert(1, "a");
    /// map.insert(2, "b");
    ///
    /// assert_eq!(map.len(), 2);
    ///
    /// map.clear();
    ///
    /// assert_eq!(map.len(), 0);
    /// ```
    #[inline]
    pub fn clear(&mut self) {
        if self.is_empty() {
            return;
        }
        self.entries.clear();
        self.index = AllocVec::with_capacity_and_populate(self.index.capacity());
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
    /// map.insert(1, "a");
    /// map.insert(2, "b");
    ///
    /// assert_eq!(map.iter().collect::<Vec<(&i32, &&str)>>(), vec![(&1, &"a"), (&2, &"b")]);
    /// ```
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.entries.iter().map(|entry| (&entry.key, &entry.value))
    }

    /// Returns a mutable iterator over the key-value pairs in the `OmniMap`.
    ///
    /// # Examples
    ///
    /// ```
    /// use omni_map::OmniMap;
    ///
    /// let mut map = OmniMap::new();
    ///
    /// map.insert(1, "a");
    /// map.insert(2, "b");
    ///
    /// for (key, value) in map.iter_mut() {
    ///     *value = "c";
    /// }
    ///
    /// assert_eq!(map.get(&1), Some(&"c"));
    /// assert_eq!(map.get(&2), Some(&"c"));
    /// ```
    #[inline]
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&K, &mut V)> {
        self.entries
            .iter_mut()
            .map(|entry| (&entry.key, &mut entry.value))
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
    /// map.insert(1, "a");
    /// map.insert(2, "b");
    ///
    /// assert_eq!(map.iter_keys().collect::<Vec<&i32>>(), vec![&1, &2]);
    /// ```
    #[inline]
    pub fn iter_keys(&self) -> impl Iterator<Item = &K> {
        self.entries.iter().map(|entry| &entry.key)
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
    /// map.insert(1, "a");
    /// map.insert(2, "b");
    ///
    /// assert_eq!(map.iter_values().collect::<Vec<&&str>>(), vec![&"a", &"b"]);
    /// ```
    #[inline]
    pub fn iter_values(&self) -> impl Iterator<Item = &V> {
        self.entries.iter().map(|entry| &entry.value)
    }

    /// Calculates the load factor of the index.
    #[inline]
    pub fn load_factor(&self) -> f64 {
        if self.index.capacity() == 0 {
            return 0.0;
        }
        self.entries.len() as f64 / self.index.capacity() as f64
    }

    /// Returns the current memory usage of the `OmniMap` in bytes.
    #[inline]
    pub fn memory_usage(&self) -> usize {
        self.entries.memory_usage() + self.index.memory_usage()
    }
}

impl<K, V> Default for OmniMap<K, V>
where
    K: Eq + Hash,
{
    /// Creates a new `OmniMap` with the default capacity.
    /// The default capacity is set to `16`.
    ///
    /// # Examples
    /// ```
    /// use omni_map::OmniMap;
    ///
    /// let map: OmniMap<i32, &str> = OmniMap::default();
    ///
    /// assert_eq!(map.capacity(), 16);
    /// ```
    #[must_use = "Creating new instances with default capacity involves allocating memory."]
    #[inline]
    fn default() -> Self {
        Self::with_capacity(16)
    }
}

impl<K, V> Index<usize> for OmniMap<K, V> {
    type Output = V;

    /// Returns immutable reference to the value at the specified index.
    ///
    /// # Parameters
    /// - `index`: The index of the value to retrieve.
    ///
    /// # Returns
    /// A reference to the value at the specified index.
    ///
    /// # Panics
    /// - if the index is out of bounds.
    ///
    /// # Examples
    /// ```
    /// use omni_map::OmniMap;
    ///
    /// let mut map = OmniMap::new();
    ///
    /// map.insert(1, "a");
    /// map.insert(2, "b");
    ///
    /// assert_eq!(map[0], "a");
    /// assert_eq!(map[1], "b");
    /// ```
    fn index(&self, index: usize) -> &Self::Output {
        // This is safe because the index is checked in the AllocVec.
        &self.entries[index].value
    }
}

impl<K, V> IndexMut<usize> for OmniMap<K, V> {
    /// Returns mutable reference to the value at the specified index.
    ///
    /// # Parameters
    /// - `index`: The index of the value to retrieve.
    ///
    /// # Returns
    /// A mutable reference to the value at the specified index.
    ///
    /// # Panics
    /// - if the index is out of bounds.
    ///
    /// # Examples
    /// ```
    /// use omni_map::OmniMap;
    ///
    /// let mut map = OmniMap::new();
    ///
    /// map.insert(1, "a");
    /// map.insert(2, "b");
    ///
    /// map[0] = "c";
    /// map[1] = "d";
    ///
    /// assert_eq!(map[0], "c");
    /// assert_eq!(map[1], "d");
    /// ```
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        // This is safe because the index is checked in the AllocVec.
        &mut self.entries[index].value
    }
}

impl<'a, K, V> IntoIterator for &'a OmniMap<K, V> {
    type Item = (&'a K, &'a V);
    type IntoIter = std::iter::Map<std::slice::Iter<'a, Entry<K, V>>, fn(&Entry<K, V>) -> (&K, &V)>;

    /// Returns an iterator over the key-value pairs in the `OmniMap`.
    fn into_iter(self) -> Self::IntoIter {
        self.entries.iter().map(|entry| (&entry.key, &entry.value))
    }
}

impl<'a, K, V> IntoIterator for &'a mut OmniMap<K, V> {
    type Item = (&'a K, &'a mut V);
    type IntoIter =
        std::iter::Map<std::slice::IterMut<'a, Entry<K, V>>, fn(&mut Entry<K, V>) -> (&K, &mut V)>;

    /// Returns a mutable iterator over the key-value pairs in the `OmniMap`.
    fn into_iter(self) -> Self::IntoIter {
        self.entries
            .iter_mut()
            .map(|entry| (&entry.key, &mut entry.value))
    }
}

pub struct OmniMapIntoIter<K, V> {
    entries: AllocVecIntoIter<Entry<K, V>>,
}

impl<K, V> Iterator for OmniMapIntoIter<K, V> {
    type Item = (K, V);

    fn next(&mut self) -> Option<Self::Item> {
        self.entries.next().map(|entry| (entry.key, entry.value))
    }
}

impl<K, V> IntoIterator for OmniMap<K, V> {
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
    ///
    /// map.insert(1, "a");
    /// map.insert(2, "b");
    ///
    /// let mut iter = map.into_iter();
    ///
    /// assert_eq!(iter.next(), Some((1, "a")));
    /// assert_eq!(iter.next(), Some((2, "b")));
    /// assert_eq!(iter.next(), None);
    /// ```
    fn into_iter(self) -> Self::IntoIter {
        OmniMapIntoIter {
           entries: self.entries.into_iter(),
        }
    }
}

impl<K, V> PartialEq for OmniMap<K, V>
where
    K: Eq,
    V: PartialEq
{
    fn eq(&self, other: &Self) -> bool {
        self.entries.eq(&other.entries) && self.index.eq(&other.index)
    }
}

impl<K, V> Clone for OmniMap<K, V>
where
    K: Clone,
    V: Clone
{
    fn clone(&self) -> Self {
        OmniMap {
            entries: self.entries.clone(),
            index: self.index.clone(),
        }
    }
}

impl<K, V> OmniMap<K, V>
where
    K: Eq + Hash + Clone, // Required to call self.reindex
    V: Clone,
{
    /// Creates a compact clone of the `OmniMap`.
    ///
    /// This method creates a clone of the `OmniMap` where the capacity of the internal
    /// storage is reduced to fit the current number of elements. This can help reduce
    /// memory usage if the map has a lot of unused capacity.
    ///
    /// # Returns
    /// A new `OmniMap` instance with the same elements as the original, but with a
    /// capacity equal to the number of elements.
    ///
    /// # Examples
    ///
    /// ```
    /// use omni_map::OmniMap;
    ///
    /// let mut map = OmniMap::with_capacity(5);
    /// map.insert(1, "a");
    /// map.insert(2, "b");
    ///
    /// let compact_clone = map.clone_compact();
    ///
    /// assert_eq!(compact_clone.len(), map.len());
    /// assert_eq!(compact_clone.capacity(), map.len());
    ///
    /// assert_eq!(compact_clone.get(&1), Some(&"a"));
    /// assert_eq!(compact_clone.get(&2), Some(&"b"));
    /// ```
    pub fn clone_compact(&self) -> Self {
        let mut clone = OmniMap {
            entries: self.entries.clone_compact(),
            index: self.index.clone_compact(), // No compaction, already fully populated.
        };
        clone.reindex(&self.len());
        clone
    }
}

impl<K, V> Debug for OmniMap<K, V>
where
    K: Eq + Hash + Debug,
    V: Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

impl<K, V> Display for OmniMap<K, V>
where
    K: Display,
    V: Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{{")?;
        for entry in &self.entries {
            writeln!(f, "    {}: {}", entry.key, entry.value)?;
        }
        write!(f, "}}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_map() {
        let map: OmniMap<String, i32> = OmniMap::new();
        assert!(map.is_empty());
        assert_eq!(map.len(), 0);
        assert_eq!(map.capacity(), 0);
    }

    #[test]
    fn test_create_map_with_capacity() {
        let map: OmniMap<String, i32> = OmniMap::with_capacity(10);
        assert!(map.is_empty());
        assert_eq!(map.len(), 0);
        assert_eq!(map.capacity(), 10);
    }

    #[test]
    fn test_omni_map_new_with_zst_ok() {
        // Zero-sized types
        let map: OmniMap<(), ()> = OmniMap::new(); // Must be Ok
        assert_eq!(map.len(), 0);
        assert_eq!(map.capacity(), 0);
    }

    #[test]
    fn test_load_factor() {
        // New map with zero capacity
        let mut map = OmniMap::new();
        assert_eq!(map.load_factor(), 0.0); // Empty map

        map.insert(1, 2);
        assert_eq!(map.load_factor(), 1.0); // Full capacity 1

        map.insert(2, 3);
        assert_eq!(map.load_factor(), 1.0); // Full capacity 2

        map.insert(3, 4);
        assert_eq!(map.load_factor(), 0.75); // 3/4 of new capacity 4, which is exactly the threshold

        map.insert(4, 5);
        assert_eq!(map.load_factor(), 1.0); // Full capacity 4

        map.insert(5, 6);
        assert_eq!(map.load_factor(), 0.625); // 5/8 of new capacity 8
    }

    #[test]
    fn test_create_map_default() {
        let map: OmniMap<String, i32> = OmniMap::default();
        assert!(map.is_empty());
        assert_eq!(map.len(), 0);
        assert_eq!(map.capacity(), 16);
    }

    #[test]
    fn test_map_insert_get() {
        let mut map = OmniMap::new();

        // Insert a key-value pair.
        // Must return None because the keys did not exist.
        assert_eq!(map.insert(1, 2), None);
        assert_eq!(map.insert(2, 3), None);

        // Length must be 2.
        assert_eq!(map.len(), 2);

        // Get the values.
        assert_eq!(map.get(&1), Some(&2));
        assert_eq!(map.get(&2), Some(&3));

        // Get nonexistent key.
        assert_eq!(map.get(&3), None);
    }

    #[test]
    fn test_map_insert_update() {
        let mut map = OmniMap::new();

        // Insert a key-value pair
        assert_eq!(map.insert(1, 2), None);
        assert_eq!(map.get(&1), Some(&2));

        // Update the value for the same key
        assert_eq!(map.insert(1, 22), Some(2));
        assert_eq!(map.get(&1), Some(&22));

        // Insert another key-value pair
        assert_eq!(map.insert(2, 3), None);
        assert_eq!(map.get(&2), Some(&3));

        // Update the value for the second key
        assert_eq!(map.insert(2, 33), Some(3));
        assert_eq!(map.get(&2), Some(&33));
    }

    #[test]
    fn test_map_get_mut() {
        let mut map = OmniMap::new();

        map.insert(1, 1);

        if let Some(value) = map.get_mut(&1) {
            *value = 10;
        }

        assert_eq!(map.get(&1), Some(&10));
    }

    #[test]
    fn test_map_access_index() {
        let mut map = OmniMap::new();
        map.insert(1, 1);
        map.insert(2, 2);
        map.insert(3, 3);

        assert_eq!(map[0], 1);
        assert_eq!(map[1], 2);
        assert_eq!(map[2], 3);

        // Remove the first item
        map.pop_front();

        // The first item now must be the second item
        // The second item now must be the third item
        assert_eq!(map[0], 2);
        assert_eq!(map[1], 3);
    }

    #[test]
    fn test_map_access_index_mut() {
        let mut map = OmniMap::new();
        map.insert(1, 1);
        map.insert(2, 2);
        map.insert(3, 3);

        map[0] = 10;
        map[1] = 20;
        map[2] = 30;

        assert_eq!(map[0], 10);
        assert_eq!(map[1], 20);
        assert_eq!(map[2], 30);
    }

    #[test]
    #[should_panic(expected = "Index out of bounds")]
    fn test_map_index_out_of_bounds() {
        let mut map = OmniMap::new();
        map.insert(1, 1);

        // ok
        assert_eq!(map[0], 1);

        // This must panic
        let _ = map[1];
    }

    #[test]
    fn test_map_insertion_order() {
        let mut map = OmniMap::new();
        map.insert(1, 1);
        map.insert(2, 2);
        map.insert(3, 3);

        assert_eq!(
            map.iter().collect::<Vec<(&i32, &i32)>>(),
            vec![(&1, &1), (&2, &2), (&3, &3)]
        );
    }

    #[test]
    fn test_map_get_first() {
        let mut map = OmniMap::new();
        map.insert(1, 1);
        map.insert(2, 2);
        map.insert(3, 3);

        assert_eq!(map.first(), Some((&1, &1)));
    }

    #[test]
    fn test_map_get_last() {
        let mut map = OmniMap::new();
        map.insert(1, 1);
        map.insert(2, 2);
        map.insert(3, 3);

        assert_eq!(map.last(), Some((&3, &3)));
    }

    #[test]
    fn test_map_pop_front() {
        let mut map = OmniMap::new();
        map.insert(1, 1); // First key
        map.insert(2, 2);
        map.insert(3, 3);

        assert_eq!(map.len(), 3);

        assert_eq!(map.pop_front(), Some((1, 1)));

        assert_eq!(map.len(), 2);
        assert_eq!(map.get(&1), None);

        // First key now must be the second key (key 2)
        assert_eq!(map.first(), Some((&2, &2)));
    }

    #[test]
    fn test_map_pop() {
        let mut map = OmniMap::new();
        map.insert(1, 1);
        map.insert(2, 2);
        map.insert(3, 3); // Last key

        assert_eq!(map.len(), 3);

        let removed_item = map.pop();

        assert_eq!(removed_item, Some((3, 3)));

        assert_eq!(map.len(), 2);

        assert_eq!(map.get(&3), None);

        // Last key now must be the second key
        assert_eq!(map.last(), Some((&2, &2)));
    }

    #[test]
    fn test_map_remove_existing_key() {
        let mut map = OmniMap::new();
        map.insert(1, 1);
        map.insert(2, 2);

        assert_eq!(map.len(), 2);

        assert_eq!(map.remove(&1), Some(1));

        assert_eq!(map.len(), 1);

        assert_eq!(map.get(&1), None);
        assert_eq!(map.get(&2), Some(&2));
    }

    #[test]
    fn test_map_remove_nonexistent_key() {
        let mut map = OmniMap::new();
        map.insert(1, 1);

        assert_eq!(map.len(), 1);

        // Must return None, because the key does not exist
        assert_eq!(map.remove(&2), None);

        assert_eq!(map.len(), 1);

        assert_eq!(map.get(&1), Some(&1));
    }

    #[test]
    fn test_map_remove_preserve_order() {
        let mut map = OmniMap::new();

        // Insert 4 items
        map.insert(1, 1);
        map.insert(2, 2);
        map.insert(3, 3);
        map.insert(4, 4);

        assert_eq!(map.len(), 4);

        // Remove the second item (key "2")
        assert_eq!(map.remove(&2), Some(2));

        assert_eq!(map.len(), 3);

        // Check the order of the remaining items
        assert_eq!(
            map.iter().collect::<Vec<(&i32, &i32)>>(),
            vec![(&1, &1), (&3, &3), (&4, &4)]
        );

        // Order of the keys must be preserved, but index has been updated
        assert_eq!(map[0], 1);
    }

    #[test]
    fn test_map_clear() {
        let mut map = OmniMap::new();
        map.insert(1, 1);

        assert_eq!(map.len(), 1);
        assert_eq!(map.capacity(), 1);

        // Clear the map
        map.clear();
        assert!(map.is_empty());

        // Capacity must not change
        assert_eq!(map.capacity(), 1);

        // Insert
        map.insert(1, 1);

        assert_eq!(map.len(), 1);
    }

    #[test]
    fn test_map_reserve_capacity() {
        let mut map = OmniMap::new();

        assert_eq!(map.capacity(), 0);

        map.insert(1, 1);

        assert_eq!(map.capacity(), 1);

        // Reserve capacity in advance
        map.reserve(10);

        // Capacity must be 11
        assert_eq!(map.capacity(), 11);

        assert_eq!(map.get(&1), Some(&1));
    }

    #[test]
    fn test_map_capacity_shrink() {
        let mut map = OmniMap::new();
        assert_eq!(map.capacity(), 0);

        for i in 0..10 {
            map.insert(i, i);
        }

        assert_eq!(map.capacity(), 16);

        map.shrink_to_fit();
        assert_eq!(map.len(), 10);
        assert_eq!(map.capacity(), 10);

        // All elements are accessible
        for i in 0..10 {
            assert_eq!(map.get(&i), Some(&i));
        }

        // Insert 5 more elements
        for i in 10..15 {
            map.insert(i, i);
        }

        assert_eq!(map.len(), 15);
        assert_eq!(map.capacity(), 32);

        // Shrink and reserve less than the current length
        map.shrink_to(10);
        assert_eq!(map.len(), 15);
        assert_eq!(map.capacity(), 32); // Capacity must stay 32

        // Shrink and reserve greater than the current length
        map.shrink_to(20);
        assert_eq!(map.len(), 15);
        assert_eq!(map.capacity(), 20); // Capacity must be adjusted to 20

        // All elements are accessible
        for i in 0..15 {
            assert_eq!(map.get(&i), Some(&i));
        }
    }

    #[test]
    fn test_into_iter_ordered() {
        let mut map = OmniMap::new();
        map.insert(1, 1);
        map.insert(2, 2);
        map.insert(3, 3);

        let mut iter = map.into_iter();

        assert_eq!(iter.next(), Some((1, 1)));
        assert_eq!(iter.next(), Some((2, 2)));
        assert_eq!(iter.next(), Some((3, 3)));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn test_map_for_loop() {
        let mut map = OmniMap::new();
        map.insert(1, 2);
        map.insert(2, 3);
        map.insert(3, 4);

        // Immutable borrow
        for (key, value) in &map {
            assert_eq!(map.get(key), Some(value));
        }
    }

    #[test]
    fn test_map_for_loop_mut() {
        let mut map = OmniMap::new();
        map.insert(1, 2);
        map.insert(2, 3);
        map.insert(3, 4);

        // Mutable borrow
        for (_, value) in &mut map {
            *value += 1;
        }

        assert_eq!(map.get(&1), Some(&3));
        assert_eq!(map.get(&2), Some(&4));
        assert_eq!(map.get(&3), Some(&5));
    }

    #[test]
    fn test_into_iterator_consumes_map() {
        let mut map = OmniMap::new();
        map.insert(1, 2);
        map.insert(2, 3);
        map.insert(3, 4);

        let mut iter: OmniMapIntoIter<i32, i32> = map.into_iter();

        assert_eq!(iter.next(), Some((1, 2)));
        assert_eq!(iter.next(), Some((2, 3)));
        assert_eq!(iter.next(), Some((3, 4)));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn test_map_clone() {
        let mut original = OmniMap::with_capacity(3);
        original.insert(1, 2);
        original.insert(2, 3);

        let mut cloned = original.clone();

        // Clone must have the same length and capacity as the original
        assert_eq!(cloned.len(), original.len());
        assert_eq!(cloned.capacity(), original.capacity());

        // Entries in the clone must be the same as in the original
        for (key, value) in original.iter() {
            assert_eq!(cloned.get(key), Some(value));
        }

        // Modifying the clone must not affect the original
        cloned.insert(3, 4);
        assert_eq!(cloned.len(), original.len() + 1);
        assert_eq!(original.len(), 2); // original length
        assert_eq!(original.get(&3), None); // Key in original does not exit
    }

    #[test]
    fn test_map_clone_compact() {
        let mut original = OmniMap::with_capacity(3);
        original.insert(1, 2);
        original.insert(2, 3);

        let mut cloned = original.clone_compact();

        // Clone must have the same length as the original
        assert_eq!(cloned.len(), original.len());

        // Clone must have a capacity equal to the length of the original
        assert_eq!(cloned.capacity(), original.len());

        // Entries in the clone must be the same as in the original
        for (key, value) in original.iter() {
            assert_eq!(cloned.get(key), Some(value));
        }

        // Modifying the clone must not affect the original
        cloned.insert(3, 4);
        assert_eq!(cloned.len(), original.len() + 1);
        assert_eq!(original.len(), 2); // original length
        assert_eq!(original.get(&3), None); // Key in original does not exit
    }

    #[test]
    fn test_map_index_integrity() {
        let mut map: OmniMap<i32, i32> = OmniMap::new();
        for i in 0..100 {
            map.insert(i, i);
        }

        for i in 0..50 {
            map.remove(&i);
        }

        // Check that the count of 'Deleted' values in index is equal to the number of deleted entries
        let deleted_count = map
            .index
            .iter()
            .filter(|&slot| matches!(slot, Slot::Deleted))
            .count();

        assert_eq!(
            deleted_count, 50,
            "Count of 'Deleted' slots in index does not match the number of deleted entries"
        );

        // Check that the count of 'Occupied' values in index is equal to the length of the map
        let occupied_count = map
            .index
            .iter()
            .filter(|&slot| matches!(slot, Slot::Occupied(_)))
            .count();

        assert_eq!(
            occupied_count,
            map.len(),
            "Count of 'Occupied' slots in index does not match the length of the map"
        );

        // Check for duplicate indices
        let mut indices = std::collections::HashSet::new();
        for slot in map.index.iter() {
            if let Slot::Occupied(index) = slot {
                assert!(indices.insert(index), "Duplicate index found: {}", index);
            }
        }
    }

    #[test]
    fn test_omni_map_debug() {
        let mut map = OmniMap::with_capacity(3);
        map.insert(1, "a");
        map.insert(2, "b");
        map.insert(3, "c");

        let debug_str = format!("{:?}", map);
        let expected_str = r#"{1: "a", 2: "b", 3: "c"}"#;

        assert_eq!(debug_str, expected_str);
    }
}
