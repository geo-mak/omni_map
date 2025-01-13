mod alloc;

use std::fmt;
use std::fmt::{Debug, Display};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::ops::{Index, IndexMut};

use crate::alloc::{BufferPointer, BufferPointerIntoIter};

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

// Required to call BufferPointer::memset_default()
impl Default for Slot {
    #[inline(always)]
    fn default() -> Self {
        Self::Empty
    }
}

/// A hybrid data structure that combines the best of both hash maps and vectors.
pub struct OmniMap<K, V> {
    entries: BufferPointer<Entry<K, V>>,
    index: BufferPointer<Slot>,
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
            // Unallocated entries and index.
            entries: BufferPointer::new(),
            index: BufferPointer::new(),
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
            entries: BufferPointer::new_allocate(capacity),
            // Initialize the index with empty slots by calling T::default()
            index: BufferPointer::new_allocate_default(capacity),
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
        self.index.count()
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

    /// Resets the index of the map with a new capacity.
    #[inline(always)]
    fn reset_index(&mut self, cap: usize) {
        self.index = BufferPointer::new_allocate_default(cap);
        self.deleted = 0;
    }

    /// Builds the index of the map according to the current entries and the capacity of the index.
    /// This method should be called **only** after resetting the index with a new capacity.
    fn build_index(&mut self) {
        let capacity = self.index.count();

        // NOTE: This must be ensured by the calling contexts, because calling this method is only
        // needed after shrinking or growing the capacity of the index.
        debug_assert_eq!(capacity, self.entries.count());

        // Build the index of the current entries.
        for (index, entry) in self.entries.iter().enumerate() {
            let mut slot_index = entry.hash % capacity;
            loop {
                let slot = self.index.load_mut(slot_index);
                match slot {
                    Slot::Empty => {
                        *slot = Slot::Occupied(index);
                        break;
                    },
                    Slot::Occupied(_) => {
                        slot_index = (slot_index + 1) % capacity;
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

    /// Shrinks or grows the allocated memory space to the specified `new_cap`.
    ///
    /// This method will also reset the index and rebuild it according to the new capacity.
    ///
    /// # Safety
    ///
    /// - Index and entries must be allocated before calling this method.
    ///
    /// - `new_cap` must be greater than `0` and within the range of `isize::MAX`.
    ///
    /// - `new_cap` must be greater than or equal to the current length.
    ///
    /// These conditions are checked in debug mode only.
    ///
    #[inline]
    fn reallocate_reindex(&mut self, new_cap: usize) {
        // Reallocate the entries with the new capacity.
        self.entries.reallocate(new_cap);
        // Reset the index with the new capacity.
        self.reset_index(self.entries.count());
        // Rebuild the index with the new capacity.
        self.build_index();
    }

    /// Deallocates the entries and the index without calling `drop` on the initialized entries.
    ///
    /// # Safety
    ///
    /// Index and entries must be allocated before calling this method.
    ///
    #[inline]
    fn deallocate_no_drop(&mut self) {
        self.entries.deallocate_no_drop();
        self.index.deallocate_no_drop();
        self.deleted = 0;
    }

    /// Resizes map with reindexing if the current load exceeds the load factor.
    fn maybe_grow(&mut self) {
        let current_cap = self.index.count();

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

            // Reallocate the entries and index with the new capacity and reindex the map.
            self.reallocate_reindex(new_cap);
        }
    }

    /// Finds the slot of the key in the index.
    ///
    /// # Returns
    ///
    /// - `(slot index, Some(index))`: If the slot is occupied and the keys match.
    ///
    /// - `(slot index, None)`: If the slot is empty or no key match is found.
    ///   The returned slot is the last checked slot before the search ends.
    ///
    fn find_slot(&self, hash: usize, key: &K) -> (usize, Option<usize>) {
        let capacity = self.index.count();
        let mut slot_index = hash % capacity;
        let mut step = 0;
        // EDGE CASE: if capacity is full and all slots are occupied, it will be an infinite loop,
        // but this is prevented by making sure that step is less than capacity.
        while step < capacity {
            match *self.index.load(slot_index) {
                Slot::Empty => {
                    // Slot is empty, key does not exist
                    return (slot_index, None);
                },
                Slot::Occupied(index) => {
                    if self.entries.load(index).key == *key {
                        return (slot_index, Some(index));
                    }
                },
                Slot::Deleted => {
                    // Deleted must be treated as occupied, because it might have been occupied
                    // by a key with the same hash, and the searched key might be in the next slot.
                },
            }
            // Search further until finding a key match or encountering an empty slot
            slot_index = (slot_index + 1) % capacity;
            step += 1;
        }
        (slot_index, None)
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

        let new_cap = self.index.count().checked_add(additional).unwrap_or(usize::MAX);

        // Reallocate the entries and index with the new capacity and reindex the map.
        self.reallocate_reindex(new_cap);
    }

    /// This method will grow the capacity of the map if the current load exceeds the load factor.
    /// If the capacity is zero, it will allocate the initial capacity without reindexing.
    #[inline(always)]
    fn ensure_capacity(&mut self) {
        if self.index.count() == 0 {
            // Allocate initial capacity for the index.
            self.index.allocate(1);
            // Fill the index with empty slots.
            self.index.memset_default();
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
                let old_value = std::mem::replace(
                    &mut self.entries.load_mut(entry_index).value, value
                );
                Some(old_value)
            }
            // No key match is found, slot is expected to be empty
            (slot_index, None) => {
                // SAFETY: The returned slot in this case is a mismatched slot that can't be safely
                // replaced with an occupied slot without extra checking.
                // The capacity-management strategy ensures that the index has empty slots,
                // otherwise the method will return the last checked slot before the search ends.
                debug_assert!(
                    matches!(self.index.load(slot_index), Slot::Empty),
                    "Logic error: slot is expected to an empty slot."
                );

                // Insert the new key-value pair
                self.entries.store_next(Entry::new(key, value, hash));
                let entry_index = self.entries.len() - 1;
                *self.index.load_mut(slot_index) = Slot::Occupied(entry_index);
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
        if self.is_empty() {
            return None;
        }

        let hash = self.hash(key);

        if let (_, Some(index)) = self.find_slot(hash, key) {
            return Some(&self.entries.load(index).value);
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
        if self.is_empty() {
            return None;
        }

        let hash = self.hash(key);

        if let (_, Some(index)) = self.find_slot(hash, key) {
            return Some(&mut self.entries.load_mut(index).value);
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
        let entry = self.entries.load_first();
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
        let entry = self.entries.load_last();
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

        // Find the slot of the key.
        if let (slot_index, Some(entry_index)) = self.find_slot(hash, key) {

            // Update index.
            *self.index.load_mut(slot_index) = Slot::Deleted;

            let entry: Entry<K, V>;

            if entry_index == self.entries.len() - 1 {

                // Remove the last entry.
                entry = self.entries.take_last();

                // Since the last entry is removed, there is no need to decrement the index.
            } else {

                // Remove the entry.
                entry = self.entries.take_shift_left(entry_index);

                // Decrement the index in all slots.
                self.decrement_index(entry_index);
            }

            // Increment the deleted counter.
            self.deleted += 1;

            // Return the value of the removed entry.
            return Some(entry.value);
        }

        // Key was not found.
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

        // SAFETY: The map is not empty, so an entry must exist.
        // Out-of-bounds check is performed in debug mode also.
        let entry_ref = self.entries.load_first();

        // Find the slot of the key.
        // Expected pattern: (slot index, Some(entry index)).
        if let (slot, Some(_)) = self.find_slot(entry_ref.hash, &entry_ref.key) {

            // Remove the first entry.
            let entry = self.entries.take_first();

            // Update the slot.
            *self.index.load_mut(slot) = Slot::Deleted;

            // Decrement the index.
            self.decrement_index(0);

            // Increment the deleted counter.
            self.deleted += 1;

            // Return the deleted entry.
            return Some((entry.key, entry.value));
        };

        // This must be unreachable, the slot must be found.
        unreachable!("Logic error: entry exists, but it has no associated slot in the index.");
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

        // SAFETY: The map is not empty, so an entry must exist.
        // Out-of-bounds check is performed in debug mode also.
        let entry_ref = self.entries.load_last();

        // Find the slot of the key.
        // Expected pattern: (slot index, Some(entry index)).
        if let (slot, Some(_)) = self.find_slot(entry_ref.hash, &entry_ref.key) {

            // Remove the last entry.
            let entry = self.entries.take_last();

            // Update the slot.
            *self.index.load_mut(slot) = Slot::Deleted;

            // Since the last entry is removed, there is no need to decrement the index.

            // Increment the deleted counter.
            self.deleted += 1;

            // Return the deleted entry.
            return Some((entry.key, entry.value));
        }

        // This must be unreachable, the slot must be found.
        unreachable!("Logic error: entry exists, but it has no associated slot in the index.");
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
        let current_len = self.entries.len();
        let current_capacity = self.index.count();

        // Capacity must be less than the current capacity and greater than or equal to the number
        // of elements.
        if capacity >= current_len && capacity < current_capacity {
            // Zero-count allocation is not allowed.
            // If the length is zero, deallocate the memory.
            if current_len > 0 {
                self.reallocate_reindex(capacity);
            } else {
                self.deallocate_no_drop();
            }
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
        let current_len = self.entries.len();

        // Capacity must be greater than the number of elements.
        if self.index.count() > current_len {
            // Zero-count allocation is not allowed.
            // If the length is zero, deallocate the memory.
            if current_len > 0 {
                self.reallocate_reindex(current_len);
            } else {
                self.deallocate_no_drop();
            }
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
        self.entries.drop_init();
        self.reset_index(self.index.count());
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

    /// Returns the current load factor of the `OmniMap` as a ratio.
    #[inline]
    pub fn current_load(&self) -> f64 {
        if self.index.count() == 0 {
            return 0.0;
        }
        (self.entries.len() + self.deleted) as f64 / self.index.count() as f64
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
        assert!(index < self.entries.len(), "Index out of bounds.");
        &self.entries.load(index).value
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
        assert!(index < self.entries.len(), "Index out of bounds.");
        &mut self.entries.load_mut(index).value
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
    entries: BufferPointerIntoIter<Entry<K, V>>,
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
            index: BufferPointer::new_allocate_default(self.entries.len()),
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
    fn test_map_current_load() {
        // New map with zero capacity
        let mut map = OmniMap::new();

        assert_eq!(map.current_load(), 0.0); // Empty map

        map.insert(1, 2);
        assert_eq!(map.current_load(), 1.0); // Full capacity 1

        map.insert(2, 3);
        assert_eq!(map.current_load(), 1.0); // Full capacity 2

        map.insert(3, 4);
        assert_eq!(map.current_load(), 0.75); // 3/4 of new capacity 4, which is exactly the threshold

        map.insert(4, 5);
        assert_eq!(map.current_load(), 1.0); // Full capacity 4

        map.insert(5, 6);
        assert_eq!(map.current_load(), 0.625); // 5/8 of new capacity 8
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

        // Access when the map is empty must return None.
        assert_eq!(map.get(&1), None);

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

        // Access when the map is empty must return None.
        assert_eq!(map.get_mut(&1), None);

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

        // Access when the map is empty must return None.
        assert_eq!(map.first(), None);

        map.insert(1, 1);
        map.insert(2, 2);
        map.insert(3, 3);

        assert_eq!(map.first(), Some((&1, &1)));
    }

    #[test]
    fn test_map_access_get_last() {
        let mut map = OmniMap::new();

        // Access when the map is empty must return None.
        assert_eq!(map.last(), None);

        map.insert(1, 1);
        map.insert(2, 2);
        map.insert(3, 3);

        assert_eq!(map.last(), Some((&3, &3)));
    }

    #[test]
    fn test_map_pop_front() {
        let mut map = OmniMap::new();

        // Pop when the map is empty must return None.
        assert_eq!(map.pop_front(), None);

        // First item.
        map.insert(1, 2);

        // Must return the only item in the map.
        let (key, value) = map.pop_front().unwrap();

        assert_eq!(key, 1);
        assert_eq!(value, 2);
        assert_eq!(map.len(), 0);
        assert_eq!(map.deleted, 1);
        assert_eq!(*map.index.load(0), Slot::Deleted);
        assert_eq!(map.capacity(), 1);

        // Must return None, because the map is empty.
        assert_eq!(map.pop_front(), None);

        // Insert new items.
        map.insert(1, 2);
        map.insert(2, 3);
        map.insert(3, 4);

        // Now, the map must expand its capacity reset the deleted counter.
        assert_eq!(map.len(), 3);
        assert_eq!(map.deleted, 0);
        assert_eq!(map.capacity(), 4);

        // Pop the first item.
        assert_eq!(map.pop_front(), Some((1, 2)));

        // Map state at this point.
        assert_eq!(map.len(), 2);
        assert_eq!(map.deleted, 1);
        assert_eq!(map.capacity(), 4);

        // Index state at this point.
        let mut deleted = 0;
        let mut occupied = 0;
        let mut empty = 0;

        for i in 0..map.index.count() {
            match map.index.load(i) {
                Slot::Deleted => {
                    deleted += 1;
                },
                Slot::Occupied(_) => {
                    occupied += 1;
                },
                Slot::Empty => {
                    empty += 1;
                }
            }
        }

        // Expected index state at this point.
        assert_eq!(deleted, 1);
        assert_eq!(occupied, 2);
        assert_eq!(empty, 1);

        // Expected values at this point.
        assert_eq!(map.get(&1), None);
        assert_eq!(map.get(&2), Some(&3));
        assert_eq!(map.get(&3), Some(&4));
    }

    #[test]
    fn test_map_pop() {
        let mut map = OmniMap::new();

        // Pop when the map is empty must return None.
        assert_eq!(map.pop(), None);

        // Last item.
        map.insert(1, 2);

        // Must return the only item in the map.
        let (key, value) = map.pop().unwrap();

        assert_eq!(key, 1);
        assert_eq!(value, 2);
        assert_eq!(map.len(), 0);
        assert_eq!(map.deleted, 1);
        assert_eq!(*map.index.load(0), Slot::Deleted);
        assert_eq!(map.capacity(), 1);

        // Must return None, because the map is empty.
        assert_eq!(map.pop(), None);

        // Insert new items.
        map.insert(1, 2);
        map.insert(2, 3);
        map.insert(3, 4);

        // Now, the map must expand its capacity reset the deleted counter.
        assert_eq!(map.len(), 3);
        assert_eq!(map.deleted, 0);
        assert_eq!(map.capacity(), 4);

        // Pop the last item.
        assert_eq!(map.pop(), Some((3, 4)));

        // Map state at this point.
        assert_eq!(map.len(), 2);
        assert_eq!(map.deleted, 1);
        assert_eq!(map.capacity(), 4);

        // Index state at this point.
        let mut deleted = 0;
        let mut occupied = 0;
        let mut empty = 0;

        for i in 0..map.index.count() {
            match map.index.load(i) {
                Slot::Deleted => {
                    deleted += 1;
                },
                Slot::Occupied(_) => {
                    occupied += 1;
                },
                Slot::Empty => {
                    empty += 1;
                }
            }
        }

        // Expected index state at this point.
        assert_eq!(deleted, 1);
        assert_eq!(occupied, 2);
        assert_eq!(empty, 1);

        // Expected values at this point.
        assert_eq!(map.get(&1), Some(&2));
        assert_eq!(map.get(&2), Some(&3));
        assert_eq!(map.get(&3), None);
    }

    #[test]
    fn test_map_remove_existing_key() {
        let mut map = OmniMap::new();

        // Remove when the map is empty must return None.
        assert_eq!(map.remove(&1), None);

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

        map.insert(1, 2);

        // Remove the only item.
        assert_eq!(map.remove(&1), Some(2));

        assert_eq!(map.len(), 0);
        assert_eq!(map.deleted, 1);
        assert_eq!(*map.index.load(0), Slot::Deleted);
        assert_eq!(map.capacity(), 1);

        // Must return None, because the map is empty.
        assert_eq!(map.remove(&1), None);

        // Insert new items.
        map.insert(1, 2);
        map.insert(2, 3);
        map.insert(3, 4);
        map.insert(4, 5);

        // Now, the map must have expanded its capacity and reset the deleted counter.
        assert_eq!(map.len(), 4);
        assert_eq!(map.deleted, 0);
        assert_eq!(map.capacity(), 4);

        // Remove the second item (key "2").
        assert_eq!(map.remove(&2), Some(3));

        // Map state at this point.
        assert_eq!(map.len(), 3);
        assert_eq!(map.deleted, 1);
        assert_eq!(map.capacity(), 4);

        // Index state at this point.
        let mut deleted = 0;
        let mut occupied = 0;
        let mut empty = 0;

        for i in 0..map.index.count() {
            match map.index.load(i) {
                Slot::Deleted => {
                    deleted += 1;
                },
                Slot::Occupied(_) => {
                    occupied += 1;
                },
                Slot::Empty => {
                    empty += 1;
                }
            }
        }

        // Expected index state at this point.
        assert_eq!(deleted, 1);
        assert_eq!(occupied, 3);
        assert_eq!(empty, 0);

        // Check the order of the remaining items.
        assert_eq!(
            map.iter().collect::<Vec<(&u8, &u8)>>(),
            vec![(&1, &2), (&3, &4), (&4, &5)]
        );

        // Order of the keys must be preserved, but index has been updated.
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

        // Remove all elements
        map.clear();

        // Length must be 0 and capacity must be 10.
        assert_eq!(map.capacity(), 10);
        assert_eq!(map.len(), 0);

        // Shrink the capacity while empty.
        // This should cause deallocation of the internal buffers.
        map.shrink_to_fit();

        assert_eq!(map.len(), 0);
        assert_eq!(map.deleted, 0);
        assert_eq!(map.capacity(), 0);
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

        // Capacity must stay 16
        assert_eq!(map.capacity(), 16);

        // Shrink and reserve greater than the current capacity (no effect)
        map.shrink_to(20);

        assert_eq!(map.len(), 10);

        // Capacity must be adjusted to 16
        assert_eq!(map.capacity(), 16);

        // Shrink and reserve less than the current capacity and greater than the current length
        map.shrink_to(12);

        assert_eq!(map.len(), 10);

        // Capacity must be adjusted to 12
        assert_eq!(map.capacity(), 12);

        // All elements are accessible
        for i in 0..10 {
            assert_eq!(map.get(&i), Some(&i));
        }

        // Remove all elements
        map.clear();

        // Length must be 0 and capacity must be 12
        assert_eq!(map.capacity(), 12);
        assert_eq!(map.len(), 0);

        // Shrink the capacity to 0 while empty.
        // This should cause deallocation of the internal buffers.
        map.shrink_to(0);

        assert_eq!(map.len(), 0);
        assert_eq!(map.deleted, 0);
        assert_eq!(map.capacity(), 0);
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
        assert_eq!(map.entries.count(), 100);
        assert_eq!(map.index.len(), 100);
        assert_eq!(map.index.count(), 100);

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
        assert_eq!(map.entries.count(), 75);
        assert_eq!(map.index.len(), 75);
        assert_eq!(map.index.count(), 75);

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
        assert_eq!(map.entries.count(), 75);
        assert_eq!(map.index.len(), 75);
        assert_eq!(map.index.count(), 75);

        // Insert new entries, the map must be able to reindex successfully
        for i in 0..100 {
            map.insert(i, i);
        }

        // Map must be reindex successfully, no deleted slots should be present
        assert!(map.index.iter().all(|slot| !matches!(slot, Slot::Deleted)));
        assert_eq!(map.deleted, 0);
        assert_eq!(map.entries.len(), 100);
        assert_eq!(map.entries.count(), 256);
        assert_eq!(map.index.len(), 256);
        assert_eq!(map.index.count(), 256);

        // Read updated keys
        for i in 0..100 {
            assert_eq!(map.get(&i), Some(&i));
        }
    }

    /// These tests check the behavior of the map when the key and value are zero-sized types.
    /// They make sure the behavior is consistent with the behavior of `HashMap` in the
    /// standard library when using zero-sized types.
    #[test]
    fn test_map_zst_keys() {
        // Grow dynamically.
        let mut map = OmniMap::new();

        // Expected op: insert.
        map.insert((), 1);

        // Expected op: update.
        map.insert((), 2);
        map.insert((), 3);

        // Len stays 1.
        assert_eq!(map.len(), 1);

        // Normally it would grow to 4, but capacity will remain invariant after the second insert.
        assert_eq!(map.capacity(), 2);

        // Access the keys returns the last updated value
        assert_eq!(map.get(&()), Some(&3));

        map.remove(&());

        // Len goes back to 0.
        assert_eq!(map.len(), 0);
        assert_eq!(map.get(&()), None);

        map.shrink_to_fit();

        // Capacity goes back to 0.
        assert_eq!(map.capacity(), 0);
    }

    #[test]
    fn test_map_zst_values() {
        // Grow dynamically.
        let mut map = OmniMap::new();

        // Add 3 items.
        map.insert(1, ());
        map.insert(2, ());
        map.insert(3, ());

        // Len and capacity as usual.
        assert_eq!(map.len(), 3);
        assert_eq!(map.capacity(), 4);

        // Access by get returns &().
        for i in 1..4 {
            assert_eq!(map.get(&i), Some(&()));
        }

        // Access by index returns ().
        for i in 0..3 {
            assert_eq!(map[i], ());
        }

        // Remove an item.
        map.remove(&2);

        assert_eq!(map.len(), 2);
        assert_eq!(map.capacity(), 4);

        map.shrink_to_fit();

        assert_eq!(map.len(), 2);
        assert_eq!(map.capacity(), 2);
    }
}
