mod alloc;

use std::fmt;
use std::fmt::{Debug, Display};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::ops::{Index, IndexMut};

use crate::alloc::{AllocVec, AllocVecIntoIter};

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
    #[inline(always)]
    fn default() -> Self {
        Self::Empty
    }
}

/// A hybrid data structure that combines the best of both hash maps and vectors.
pub struct OmniMap<K, V> {
    entries: AllocVec<Entry<K, V>>,
    index: AllocVec<Slot>,
    deleted: usize,
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
            deleted: 0,
        }
    }

    /// Creates a new `OmniMap` with the specified capacity.
    ///
    /// # Parameters
    ///
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
            deleted: 0,
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
    ///
    /// # Returns
    ///
    /// - `(slot, Some(index))`: If the slot is occupied and the keys match.
    ///
    /// - `(slot, None)`: If the slot is empty or no key match is found.
    ///   The returned slot is the last checked slot before the search ends.
    ///
    fn find_slot(&self, hash: usize, key: &K) -> (usize, Option<usize>) {
        let capacity = self.index.capacity();
        let mut slot = hash % capacity;
        let mut step = 0;
        // EDGE CASE: if capacity is full and all slots are occupied, it will be an infinite loop,
        // but this is prevented by making sure that step is less than capacity.
        while step < capacity {
            match self.index[slot] {
                Slot::Empty => {
                    // Slot is empty, key does not exist
                    return (slot, None);
                },
                Slot::Occupied(index) => {
                    if self.entries[index].key == *key {
                        return (slot, Some(index));
                    }
                },
                Slot::Deleted => {
                    // Deleted must be treated as occupied, because it might have been occupied
                    // by a key with the same hash, and the searched key might be in the next slot.
                },
            }
            // Search further until finding a key match or encountering an empty slot
            slot = (slot + 1) % capacity;
            step += 1;
        }
        (slot, None)
    }

    /// Resets the index of the map with a new capacity.
    #[inline(always)]
    fn reset_index(&mut self, cap: usize) {
        self.index = AllocVec::with_capacity_and_populate(cap);
        self.deleted = 0;
    }

    /// Builds the index of the map according to the current entries and the capacity of the index.
    /// This method should be called **only** after resetting the index with a new capacity.
    fn build_index(&mut self) {
        let capacity = self.index.capacity();

        // NOTE: This must be ensured by the calling contexts, because calling this method is only
        // needed after shrinking or growing the capacity of the index.
        debug_assert_eq!(capacity, self.entries.capacity());

        // Build the index of the current entries.
        for (index, entry) in self.entries.iter().enumerate() {
            let mut slot = entry.hash % capacity;
            loop {
                match self.index[slot] {
                    Slot::Empty => {
                        self.index[slot] = Slot::Occupied(index);
                        break;
                    },
                    Slot::Occupied(_) => {
                        slot = (slot + 1) % capacity;
                    },
                    Slot::Deleted => {
                        panic!("Logic error: deleted slot found in the index.");
                    },

                }
            }
        }
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

    /// Grows the capacity of the map with reindexing.
    ///
    /// # Safety
    ///
    /// This method should be called only after prior allocation and after ensuring that the
    /// `new_cap` is greater than the current capacity.
    ///
    /// # Parameters
    ///
    /// - `new_cap`: The new capacity of the map.
    ///
    fn grow_reindex(&mut self, new_cap: usize) {
        // This must be ensured by the calling contexts.
        debug_assert!(
            new_cap > self.capacity(),
            "Logic error: new capacity must be larger than the current capacity."
        );

        // Reallocate the entries with the new capacity.
        // SAFETY: This call is assumed to be safe if the new capacity is greater than the current
        // capacity.
        self.entries.reallocate(new_cap);
        // Reset the index with the new capacity.
        self.reset_index(self.entries.capacity());
        // Rebuild the index with the new capacity.
        self.build_index();
    }

    /// Resizes map with reindexing if the current load exceeds the load factor.
    fn maybe_grow(&mut self) {
        let current_cap = self.index.capacity();

        let load_factor = (self.entries.len() + self.deleted) as f64 / current_cap as f64;

        // If the current load exceeds the load factor, grow the capacity.
        if load_factor > Self::LOAD_FACTOR {
            let growth_factor = (current_cap as f64 / Self::LOAD_FACTOR).ceil() as usize;

            // New capacity must be within the range of `usize` and less than or equal to
            // `isize::MAX` when rounded up to the nearest multiple of `align` to ensure successful
            // allocation.
            // Error-handling is not needed because there is no way to communicate these errors to
            // the caller without making insert, reserve, etc. return a Result.
            let new_cap = growth_factor
                .checked_next_power_of_two()
                .unwrap_or(usize::MAX);

            // Allocate the additional capacity with reindexing
            self.grow_reindex(new_cap);
        }
    }

    /// Reserves capacity for `additional` more elements.
    /// The resulting capacity will be equal to `self.capacity() + additional` exactly.
    ///
    /// # Time Complexity
    ///
    /// _O_(n) on average, where *n* is the number of elements in the map.
    ///
    /// # Parameters
    ///
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
        self.grow_reindex(self.index.capacity() + additional);
    }

    /// This method will grow the capacity of the map if the current load exceeds the load factor.
    /// If the capacity is zero, it will allocate the initial capacity without reindexing.
    #[inline(always)]
    fn ensure_capacity(&mut self) {
        // Ensure that the map has enough capacity to insert the new key-value pair.
        if self.index.capacity() == 0 {
            // Allocate initial capacity for the index.
            self.index.allocate(1);
            // Fill the index with empty slots.
            self.index.memset_f(Slot::default);
            // Allocate initial capacity for the entries.
            self.entries.allocate(1);
        } else {
            // This will reindex the map if the capacity is grown.
            self.maybe_grow();
        }
    }

    /// Inserts a key-value pair into the map.
    /// If the map did not have this key present, `None` is returned.
    /// If the map did have this key present, the value is updated, and the old value is returned.
    ///
    /// # Parameters
    ///
    /// - `key`: The key to insert or update.
    ///
    /// - `value`: The value to associate with the key.
    ///
    /// # Time Complexity
    ///
    /// Amortized _O_(1).
    ///
    /// # Examples
    ///
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
        self.ensure_capacity();

        // Hash the key
        let hash = self.hash(&key);

        // This is safe because empty slots are guaranteed to exist.
        match self.find_slot(hash, &key) {
            // A key match is found
            (_, Some(entry_index)) => {
                // Key exists, update the value
                let old_value = std::mem::replace(&mut self.entries[entry_index].value, value);
                Some(old_value)
            }
            // No key match is found, slot is expected to be empty
            (slot_index, None) => {
                // SAFETY: The returned slot in this case is a mismatched slot that can't be safely
                // replaced with an occupied slot without extra checking.
                // The capacity-management strategy ensures that the index has empty slots,
                // otherwise the method will return the last checked slot before the search ends.
                debug_assert!(
                    matches!(self.index[slot_index], Slot::Empty),
                    "Logic error: slot is expected to an empty slot."
                );

                // Insert the new key-value pair
                self.entries.push_no_grow(Entry::new(key, value, hash));
                let entry_index = self.entries.len() - 1;
                self.index[slot_index] = Slot::Occupied(entry_index);
                None
            },
        }
    }

    /// Retrieves a value by its key.
    ///
    /// # Parameters
    ///
    /// - `key`: The key for which to retrieve the value.
    ///
    /// # Returns
    ///
    /// - `Some(&value)`: if the key is found.
    ///
    /// - `None`: if the key does not exist.
    ///
    /// # Time Complexity
    ///
    /// _O_(1) on average.
    ///
    /// # Examples
    ///
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
        if let (_, Some(index)) = self.find_slot(hash, key) {
            return Some(&self.entries[index].value);
        }
        None
    }

    /// Retrieves a mutable reference to a value by its key.
    ///
    /// # Parameters
    ///
    /// - `key`: The key for which to retrieve the mutable reference to the value.
    ///
    /// # Returns
    ///
    /// - `Some(&mut value)`: If the key is found.
    ///
    /// - `None`: If the key does not exist.
    ///
    /// # Time Complexity
    ///
    /// _O_(1) on average.
    ///
    /// # Examples
    ///
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
        if let (_, Some(index)) = self.find_slot(hash, key) {
            return Some(&mut self.entries[index].value);
        }
        None
    }

    /// Returns the first entry in the map.
    ///
    /// # Returns
    ///
    /// - `Some((&key, &value))`: If the map is not empty.
    ///
    /// - `None`: If the map is empty.
    ///
    /// # Time Complexity
    ///
    /// _O_(1).
    ///
    /// # Examples
    ///
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
    ///
    /// - `Some((&key, &value))`: If the map is not empty.
    ///
    /// - `None`: If the map is empty.
    ///
    /// # Time Complexity
    ///
    /// _O_(1).
    ///
    /// # Examples
    ///
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
    ///
    /// - `key`: The key to remove.
    ///
    /// # Returns
    ///
    /// - `Some(value)`: If the key is found and removed.
    ///
    /// - `None`: If the key does not exist.
    ///
    /// # Time Complexity
    ///
    /// - _O_(n) in the worst case.
    ///
    /// - _O_(1) if the entry is the last entry.
    ///
    /// # Note
    /// This method does not shrink the current capacity of the map.
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

        // Find the slot of the key
        if let (slot, Some(index)) = self.find_slot(hash, key) {
            let entry: Entry<K, V>;

            // Call remove or pop based on the index
            if index == self.entries.len() - 1 {
                // This is safe because the map is not empty
                entry = self.entries.pop();
                self.index[slot] = Slot::Deleted;
            } else {
                entry = self.entries.remove(index);
                self.index[slot] = Slot::Deleted;
                self.decrement_index(index);
            }

            // Add the deleted slot to the deleted counter
            self.deleted += 1;

            // Return the value of the removed entry
            return Some(entry.value);
        }
        None
    }

    /// Pops the first entry from the map.
    /// The capacity of the map remains unchanged.
    ///
    /// # Returns
    ///
    /// - `Some((key, value))`: If the map is not empty.
    ///
    /// - `None`: If the map is empty.
    ///
    /// # Time Complexity
    ///
    /// _O_(n) in the worst case.
    ///
    /// # Examples
    ///
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
        // Add the deleted slot to the deleted counter
        self.deleted += 1;
        Some((entry.key, entry.value))
    }

    /// Pops the last entry from the map.
    /// The capacity of the map remains unchanged.
    ///
    /// # Returns
    ///
    /// - `Some((key, value))`: If the map is not empty.
    ///
    /// - `None`: If the map is empty.
    ///
    /// # Time Complexity
    ///
    /// _O_(1) on average.
    ///
    /// # Examples
    ///
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
        if let (slot, Some(_)) = self.find_slot(entry.hash, &entry.key) {
            self.index[slot] = Slot::Deleted;
            // This is safe because the map is not empty
            let entry = self.entries.pop();
            // Add the deleted slot to the deleted counter
            self.deleted += 1;
            return Some((entry.key, entry.value));
        }
        None
    }

    /// Shrinks the capacity of the `OmniMap` to the specified capacity.
    /// In order to take effect, `capacity` must be less than the current capacity
    /// and greater than or equal to the number of elements in the map.
    ///
    /// # Parameters
    ///
    /// - `capacity`: The new capacity of the map.
    ///
    /// # Time Complexity
    ///
    /// _O_(n) on average, where *n* is the number of elements in the map.
    ///
    /// # Examples
    ///
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
        // Capacity must be less than the current capacity and greater than or equal to the number
        // of elements.
        if capacity < self.index.capacity() && capacity >= self.entries.len() {
            // NOTE: This call is safe, because its conditions are checked already.
            self.entries.reallocate(capacity);
            // Reset the index with the new capacity.
            self.reset_index(capacity);
            // Rebuild the index with the new capacity.
            self.build_index();
        }
    }

    /// Shrinks the capacity of the `OmniMap` to fit its current length.
    /// If the capacity is equal to the number of elements in the map, this method will do nothing.
    ///
    /// # Time Complexity
    ///
    /// _O_(n) on average, where *n* is the number of elements in the map.
    ///
    /// # Examples
    ///
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
        // Capacity must be greater than the number of elements.
        if self.index.capacity() > self.entries.len() {
            // NOTE: This call is safe, because its condition is checked already.
            self.entries.reallocate(self.entries.len());
            // Reset the index with the new capacity.
            self.reset_index(self.entries.len());
            // Rebuild the index with the new capacity.
            self.build_index();
        }
    }

    /// Clears the map, removing all key-value pairs.
    /// The capacity of the map remains unchanged.
    ///
    /// # Time Complexity
    ///
    /// _O_(n).
    ///
    /// # Examples
    ///
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
        self.reset_index(self.index.capacity());
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
        (self.entries.len() + self.deleted) as f64 / self.index.capacity() as f64
    }

    /// Returns the current memory usage of the `OmniMap` in bytes.
    #[inline]
    pub fn memory_usage(&self) -> usize {
        self.entries.memory_usage() + self.index.memory_usage() + size_of::<usize>()
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
    ///
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
    ///
    /// - `index`: The index of the value to retrieve.
    ///
    /// # Returns
    ///
    /// A reference to the value at the specified index.
    ///
    /// # Panics
    ///
    /// If the index is out of bounds.
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
    ///
    /// - `index`: The index of the value to retrieve.
    ///
    /// # Returns
    ///
    /// A mutable reference to the value at the specified index.
    ///
    /// # Panics
    ///
    /// If the index is out of bounds.
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
    ///
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
            deleted: self.deleted,
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
    ///
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
            // Clone the entries with compact capacity
            entries: self.entries.clone_compact(),
            // NOTE: Index can't be compacted because it's length is equal to the capacity,
            // so we allocate a new index with capacity equal to the number of elements.
            index: AllocVec::with_capacity_and_populate(self.entries.len()),
            deleted: 0,
        };
        clone.build_index();
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
    fn test_map_new() {
        let map: OmniMap<u8, &str> = OmniMap::new();

        assert!(map.is_empty());
        assert_eq!(map.len(), 0);
        assert_eq!(map.deleted, 0);
        assert_eq!(map.capacity(), 0);
    }

    #[test]
    fn test_map_new_with_capacity() {
        let map: OmniMap<u8, &str> = OmniMap::with_capacity(10);

        assert!(map.is_empty());
        assert_eq!(map.len(), 0);
        assert_eq!(map.deleted, 0);
        assert_eq!(map.capacity(), 10);
    }

    #[test]
    fn test_map_load_factor() {
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
    fn test_map_new_default() {
        let map: OmniMap<u8, &str> = OmniMap::default();

        assert!(map.is_empty());
        assert_eq!(map.len(), 0);
        assert_eq!(map.deleted, 0);
        assert_eq!(map.capacity(), 16);
    }

    #[test]
    fn test_map_insert_get() {
        let mut map = OmniMap::new();

        // Insert
        assert_eq!(map.insert(1, 2), None);
        assert_eq!(map.insert(2, 3), None);
        assert_eq!(map.insert(3, 4), None);

        // Map state
        assert!(!map.is_empty());
        assert_eq!(map.len(), 3);
        assert_eq!(map.deleted, 0);
        assert_eq!(map.capacity(), 4);

        // Check values
        assert_eq!(map.get(&1), Some(&2));
        assert_eq!(map.get(&2), Some(&3));
        assert_eq!(map.get(&3), Some(&4));
    }

    #[test]
    fn test_map_insert_update() {
        let mut map = OmniMap::new();

        // Insert a key-value pair
        map.insert(1, 2);
        map.insert(2, 3);
        map.insert(3, 4);

        // Update the value for keys 1 and 2
        assert_eq!(map.insert(1, 22), Some(2));
        assert_eq!(map.insert(2, 33), Some(3));

        // Values must be updated
        assert_eq!(map.get(&1), Some(&22));
        assert_eq!(map.get(&2), Some(&33));

        // Key 3 must remain the same
        assert_eq!(map.get(&3), Some(&4));
    }

    #[test]
    fn test_map_access_get_mut() {
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
    fn test_map_access_index_out_of_bounds() {
        let mut map = OmniMap::new();

        map.insert(1, 1);

        // ok
        assert_eq!(map[0], 1);

        // This must panic
        let _ = map[1];
    }

    #[test]
    fn test_map_access_get_first() {
        let mut map = OmniMap::new();

        map.insert(1, 1);
        map.insert(2, 2);
        map.insert(3, 3);

        assert_eq!(map.first(), Some((&1, &1)));
    }

    #[test]
    fn test_map_access_get_last() {
        let mut map = OmniMap::new();

        map.insert(1, 1);
        map.insert(2, 2);
        map.insert(3, 3);

        assert_eq!(map.last(), Some((&3, &3)));
    }

    #[test]
    fn test_map_pop_front() {
        let mut map = OmniMap::new();

        // Insert 3 items
        map.insert(1, 2); // First key
        map.insert(2, 3);
        map.insert(3, 4);

        assert_eq!(map.len(), 3);
        assert_eq!(map.deleted, 0);
        assert_eq!(map.capacity(), 4);

        // Pop the first item
        assert_eq!(map.pop_front(), Some((1, 2)));

        assert_eq!(map.len(), 2);
        assert_eq!(map.deleted, 1);
        assert_eq!(map.capacity(), 4);

        // Access by get to the remaining items
        assert_eq!(map.get(&1), None);
        assert_eq!(map.get(&2), Some(&3));
        assert_eq!(map.get(&3), Some(&4));
    }

    #[test]
    fn test_map_pop() {
        let mut map = OmniMap::new();

        // Insert 3 items
        map.insert(1, 2);
        map.insert(2, 3);
        map.insert(3, 4); // Last key

        assert_eq!(map.len(), 3);
        assert_eq!(map.deleted, 0);
        assert_eq!(map.capacity(), 4);

        let removed_item = map.pop();

        assert_eq!(removed_item, Some((3, 4)));

        assert_eq!(map.len(), 2);
        assert_eq!(map.deleted, 1);
        assert_eq!(map.capacity(), 4);

        // Access by get to the remaining items
        assert_eq!(map.get(&1), Some(&2));
        assert_eq!(map.get(&2), Some(&3));
        assert_eq!(map.get(&3), None);
    }

    #[test]
    fn test_map_remove_existing_key() {
        let mut map = OmniMap::new();

        // Insert 4 items
        map.insert(1, 2);
        map.insert(2, 3);
        map.insert(3, 4);

        assert_eq!(map.len(), 3);
        assert_eq!(map.deleted, 0);
        assert_eq!(map.capacity(), 4);

        assert_eq!(map.remove(&1), Some(2));

        assert_eq!(map.len(), 2);
        assert_eq!(map.deleted, 1);
        assert_eq!(map.capacity(), 4);

        // Access by get to the remaining items
        assert_eq!(map.get(&1), None);
        assert_eq!(map.get(&2), Some(&3));
        assert_eq!(map.get(&3), Some(&4));
    }

    #[test]
    fn test_map_remove_preserve_order() {
        let mut map = OmniMap::new();

        // Insert 4 items
        map.insert(1, 2);
        map.insert(2, 3);
        map.insert(3, 4);
        map.insert(4, 5);

        assert_eq!(map.len(), 4);
        assert_eq!(map.deleted, 0);

        // Remove the second item (key "2")
        assert_eq!(map.remove(&2), Some(3));

        assert_eq!(map.len(), 3);
        assert_eq!(map.deleted, 1);
        assert_eq!(map.capacity(), 4);

        // Check the order of the remaining items
        assert_eq!(
            map.iter().collect::<Vec<(&u8, &u8)>>(),
            vec![(&1, &2), (&3, &4), (&4, &5)]
        );

        // Order of the keys must be preserved, but index has been updated
        assert_eq!(map[0], 2);
        assert_eq!(map[1], 4);
        assert_eq!(map[2], 5);
    }

    #[test]
    fn test_map_remove_nonexistent_key() {
        let mut map = OmniMap::new();

        map.insert(1, 1);

        assert_eq!(map.len(), 1);
        assert_eq!(map.deleted, 0);
        assert_eq!(map.capacity(), 1);

        // Must return None, because the key does not exist
        assert_eq!(map.remove(&2), None);

        assert_eq!(map.len(), 1);
        assert_eq!(map.deleted, 0);
        assert_eq!(map.capacity(), 1);

        assert_eq!(map.get(&1), Some(&1));
    }

    #[test]
    fn test_map_clear() {
        let mut map = OmniMap::with_capacity(4);

        map.insert(1, 2);
        map.insert(2, 3);
        map.insert(3, 4);

        assert_eq!(map.len(), 3);
        assert_eq!(map.deleted, 0);
        assert_eq!(map.capacity(), 4);

        // Remove an item
        map.remove(&1);

        assert_eq!(map.len(), 2);
        assert_eq!(map.deleted, 1);
        assert_eq!(map.capacity(), 4);

        // Clear the map
        map.clear();

        assert_eq!(map.len(), 0);
        assert_eq!(map.deleted, 0);
        assert_eq!(map.capacity(), 4);

        // Insert again
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

        // Capacity must be 1 + requested capacity = 11
        assert_eq!(map.capacity(), 11);

        assert_eq!(map.get(&1), Some(&1));
    }

    #[test]
    fn test_map_capacity_shrink_to_fit() {
        let mut map = OmniMap::new();

        assert_eq!(map.capacity(), 0);

        for i in 0..10 {
            map.insert(i, i);
        }

        assert_eq!(map.capacity(), 16);

        // Shrink the capacity to the current length
        map.shrink_to_fit();

        assert_eq!(map.len(), 10);
        assert_eq!(map.deleted, 0);
        assert_eq!(map.capacity(), 10);

        // All elements are accessible
        for i in 0..10 {
            assert_eq!(map.get(&i), Some(&i));
        }
    }

    #[test]
    fn test_map_capacity_shrink_to() {
        let mut map = OmniMap::new();

        assert_eq!(map.capacity(), 0);

        for i in 0..10 {
            map.insert(i, i);
        }

        assert_eq!(map.len(), 10);
        assert_eq!(map.deleted, 0);
        assert_eq!(map.capacity(), 16);

        // Shrink and reserve less than the current length (no effect)
        map.shrink_to(5);

        assert_eq!(map.len(), 10);
        assert_eq!(map.capacity(), 16); // Capacity must stay 16

        // Shrink and reserve greater than the current capacity (no effect)
        map.shrink_to(20);

        assert_eq!(map.len(), 10);
        assert_eq!(map.capacity(), 16); // Capacity must be adjusted to 16

        // Shrink and reserve less than the current capacity and greater than the current length
        map.shrink_to(12);

        assert_eq!(map.len(), 10);
        assert_eq!(map.capacity(), 12); // Capacity must be adjusted to 12

        // All elements are accessible
        for i in 0..10 {
            assert_eq!(map.get(&i), Some(&i));
        }
    }

    #[test]
    fn test_map_into_iter_order() {
        let mut map = OmniMap::new();

        map.insert(1, 2);
        map.insert(2, 3);
        map.insert(3, 4);

        let mut iter = map.into_iter();

        assert_eq!(iter.next(), Some((1, 2)));
        assert_eq!(iter.next(), Some((2, 3)));
        assert_eq!(iter.next(), Some((3, 4)));
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
    fn test_map_into_iter_consume() {
        let mut map = OmniMap::new();

        map.insert(1, 2);
        map.insert(2, 3);
        map.insert(3, 4);

        let mut iter: OmniMapIntoIter<u8, u8> = map.into_iter();

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
        assert_eq!(cloned.deleted, original.deleted);
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
        original.insert(3, 4);

        // Remove the last item
        original.pop();

        let mut cloned = original.clone_compact();

        // Clone must have the same length as the original
        assert_eq!(cloned.len(), original.len());

        // Deleted slots must be removed in the clone
        assert_ne!(cloned.deleted, original.deleted);

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
    fn test_map_debug() {
        let mut map = OmniMap::with_capacity(3);

        map.insert(1, "a");
        map.insert(2, "b");
        map.insert(3, "c");

        let debug_str = format!("{:?}", map);
        let expected_str = r#"{1: "a", 2: "b", 3: "c"}"#;

        assert_eq!(debug_str, expected_str);
    }

    #[test]
    fn test_map_index_integrity() {
        let mut map= OmniMap::with_capacity(100);

        // Initial state, all slots must be empty
        assert!(map.index.iter().all(|slot| matches!(slot, Slot::Empty)));
        assert_eq!(map.deleted, 0);
        assert_eq!(map.entries.len(), 0);
        assert_eq!(map.entries.capacity(), 100);
        assert_eq!(map.index.len(), 100);
        assert_eq!(map.index.capacity(), 100);

        // Full capacity
        for i in 0..100 {
            assert_eq!(map.insert(i, i), None);
        }

        // Remove some entries
        for i in 75..100 {
            assert_eq!(map.remove(&i), Some(i));
        }

        // Collect slots' information
        let mut occupied_indices = std::collections::HashSet::new();
        let mut empty_indices = 0;
        let mut deleted_indices = 0;

        for slot in map.index.iter() {
            match slot {
                Slot::Occupied(index) => {
                    assert!(occupied_indices.insert(index), "Duplicate index found: {}", index);
                }
                Slot::Empty => {
                    empty_indices += 1;
                }
                Slot::Deleted => {
                    deleted_indices += 1;
                }
            }
        }

        // Check integrity
        assert_eq!(occupied_indices.len(), 75);
        assert_eq!(deleted_indices, 25);
        assert_eq!(empty_indices, map.capacity() - (occupied_indices.len() + deleted_indices));

        // Compact the map to reindex
        map.shrink_to_fit();

        // No deleted slots should be present, all slots must be occupied
        assert!(map.index.iter().all(|slot| matches!(slot, Slot::Occupied(_))));
        assert_eq!(map.deleted, 0);
        assert_eq!(map.entries.len(), 75);
        assert_eq!(map.entries.capacity(), 75);
        assert_eq!(map.index.len(), 75);
        assert_eq!(map.index.capacity(), 75);

        // Update entries
        for i in 0..50 {
            map.insert(i, i * 2);
        }

        // Read updated entries
        for i in 0..50 {
            assert_eq!(map.get(&i), Some(&(i * 2)));
        }

        // Compact the map to reindex
        map.shrink_to_fit();

        // Remove all entries
        for i in 0..75 {
            map.remove(&i);
        }

        // No occupied or empty slots should be present, all slots must be deleted
        assert!(map.index.iter().all(|slot| matches!(slot, Slot::Deleted)));
        assert_eq!(map.deleted, 75);
        assert_eq!(map.entries.len(), 0);
        assert_eq!(map.entries.capacity(), 75);
        assert_eq!(map.index.len(), 75);
        assert_eq!(map.index.capacity(), 75);

        // Insert new entries, the map must be able to reindex successfully
        for i in 0..100 {
            map.insert(i, i);
        }

        // Map must be reindex successfully, no deleted slots should be present
        assert!(map.index.iter().all(|slot| !matches!(slot, Slot::Deleted)));
        assert_eq!(map.deleted, 0);
        assert_eq!(map.entries.len(), 100);
        assert_eq!(map.entries.capacity(), 256);
        assert_eq!(map.index.len(), 256);
        assert_eq!(map.index.capacity(), 256);

        // Read updated keys
        for i in 0..100 {
            assert_eq!(map.get(&i), Some(&i));
        }
    }
}
