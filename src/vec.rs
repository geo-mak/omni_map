use std::alloc::{self, Layout};
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut, Index, IndexMut, Range};
use std::ptr::{self, NonNull};

/// Raw vector to enable better control over memory allocation.
/// It relies on explicit calls to `reserve` to grow, otherwise,
/// it will only allocate space for one element each time `push` is called.
///
/// Internal representation:
///
/// - `ptr` is a non-null pointer to the first element of the vector.
/// - `len` is the number of elements in the vector.
/// - `cap` is the number of elements the vector can hold.
///
/// ```text
///            ptr  +  len   +  cap     --
///      NonNull<T> |  usize |  usize     |
///        +--------+--------+--------+   |
///        | 0x0123 |      2 |      4 |   |--> Metadata
///        +--------+--------+--------+   |
///             |                       --
///             v                                --
///        +--------+--------+--------+--------+   |
///        | val: T | val: T | uninit | uninit |   |--> Heap Layout::array::<T>(cap)
///        +--------+--------+--------+--------+   |
///          0x0123   0x0127   0x012B   0x012F     |    Alignment is illustrative
///             0       1        2        3      --
///
/// ```
///
#[derive(Debug, PartialEq)]
pub(crate) struct AllocVec<T> {
    ptr: NonNull<T>,
    ///
    /// # Safety
    ///
    /// `cap` must be in the `0..=usize::MAX` range.
    cap: usize,
    len: usize,
    _marker: PhantomData<T>,
}

impl<T> AllocVec<T> {
    /// Creates a new, empty `AllocVec`.
    #[must_use]
    #[inline]
    pub(crate) fn new() -> Self {
        AllocVec {
            ptr: NonNull::dangling(),
            cap: 0,
            len: 0,
            _marker: PhantomData,
        }
    }

    /// Creates a new `AllocVec` with the specified capacity.
    ///
    /// # Arguments
    ///
    /// * `cap` - The capacity of the new `AllocVec`.
    #[must_use]
    #[inline]
    pub(crate) fn with_capacity(cap: usize) -> Self {
        if cap == 0 {
            return Self::new();
        }
        let layout = Layout::array::<T>(cap).expect("Allocation error: layout error");

        let ptr = unsafe { alloc::alloc(layout) as *mut T };

        let ptr = NonNull::new(ptr).expect("Allocation error: pointer is null");

        AllocVec {
            ptr,
            cap,
            len: 0,
            _marker: PhantomData,
        }
    }

    /// Returns the capacity of the `AllocVec`.
    #[inline]
    pub(crate) fn capacity(&self) -> usize {
        self.cap
    }

    /// Returns the length of the `AllocVec`.
    #[inline]
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    /// Returns `true` if the `AllocVec` is empty.
    #[inline]
    fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Allocates or reallocates the vector to a new capacity.
    fn allocate(&mut self, new_cap: usize) {
        let new_layout = Layout::array::<T>(new_cap).expect("Allocation error: layout error");
        let new_ptr = if self.cap == 0 {
            // Allocate new memory space
            unsafe { alloc::alloc(new_layout) as *mut T }
        } else {
            // Reallocate the memory space with the new capacity
            let old_layout = Layout::array::<T>(self.cap).expect("Allocation error: layout error");
            unsafe {
                alloc::realloc(self.ptr.as_ptr() as *mut u8, old_layout, new_layout.size())
                    as *mut T
            }
        };
        self.ptr = NonNull::new(new_ptr).expect("Allocation error: pointer is null");
        self.cap = new_cap;
    }

    /// Reserves capacity for at least `additional` more elements.
    /// The resulted capacity will be `self.capacity() + additional`.
    ///
    /// # Arguments
    ///
    /// * `additional` - The number of additional elements to reserve space for.
    ///
    /// # Panics
    ///
    /// Panics if the new capacity overflows `usize`.
    ///
    /// # Time Complexity
    /// - *O*(n) where n is the new capacity.
    ///
    #[inline]
    pub(crate) fn reserve(&mut self, additional: usize) {
        // Check for capacity overflow
        let new_cap = self.cap.checked_add(additional).expect("capacity overflow");
        if new_cap > self.cap {
            self.allocate(new_cap);
        }
    }

    /// Shrinks the capacity of the `AllocVec` to the specified capacity.
    ///
    /// # Arguments
    ///
    /// * `new_cap` - The new capacity of the `AllocVec`.
    ///
    /// # Time Complexity
    /// - *O*(n) where n is the new capacity of the `AllocVec`.
    ///
    #[inline]
    pub(crate) fn shrink_to(&mut self, new_cap: usize) {
        if new_cap < self.cap && new_cap >= self.len {
            self.allocate(new_cap);
        }
    }

    /// Shrinks the capacity of the `AllocVec` to match its current length.
    ///
    /// This method reallocates the internal buffer to fit exactly the number of elements currently
    /// stored in the `AllocVec`. If the current capacity is already equal to the length, this method
    /// does nothing.
    ///
    /// # Panics
    ///
    /// This method will panic if the new layout for the reallocation cannot be created.
    ///
    /// # Time Complexity
    /// - *O*(n) where n is the length of the `AllocVec`.
    ///
    #[inline]
    pub(crate) fn shrink_to_fit(&mut self) {
        if self.cap > self.len {
            self.allocate(self.len);
        }
    }

    /// Resizes the `AllocVec` to the specified length, using the provided function to generate new elements.
    /// If the new length is less than the current length, the elements at the end of the `AllocVec` are dropped,
    /// and elements in the range 0..new_len are overwritten.
    ///
    /// # Note
    ///
    /// Resizing to a smaller length will not cause vector to shrink capacity.
    /// Allocated capacity will remain the same.
    ///
    ///
    /// # Arguments
    ///
    /// * `new_len` - The new length of the `AllocVec`.
    /// * `f` - The function to generate new elements.
    ///
    /// # Time Complexity
    /// - *O*(n) where n is the new length of the `AllocVec`.
    ///
    pub(crate) fn resize_with<F>(&mut self, new_len: usize, mut f: F)
    where
        F: FnMut() -> T,
    {
        if new_len > self.len {
            // Reserve space if needed
            if new_len > self.cap {
                self.reserve(new_len - self.len);
            }
            for i in self.len..new_len {
                unsafe {
                    ptr::write(self.ptr.as_ptr().add(i), f());
                }
            }
            // Update length
            self.len = new_len;
        } else if new_len < self.len {
            unsafe {
                // Drop elements in the range to their release resources
                ptr::drop_in_place(std::slice::from_raw_parts_mut(
                    self.ptr.as_ptr().add(new_len),
                    self.len - new_len,
                ));
                // Write new elements in the range 0..new_len
                for i in 0..new_len {
                    ptr::write(self.ptr.as_ptr().add(i), f());
                }
            }
            self.len = new_len;
        }
    }

    /// Appends an element to the back of the `AllocVec`.
    ///
    /// # Arguments
    ///
    /// * `value` - The value to append.
    ///
    /// # Time Complexity
    /// - Amortized *O*(1).
    ///
    #[inline]
    pub(crate) fn push(&mut self, value: T) {
        if self.len == self.cap {
            self.reserve(1);
        }
        unsafe {
            ptr::write(self.ptr.as_ptr().add(self.len), value);
        }
        // Update length
        self.len += 1;
    }

    /// Attempts to append an element to the back of the `AllocVec`.
    ///
    /// If the `AllocVec` has reached its capacity, the method returns an `Err` containing the value.
    ///
    /// # Arguments
    ///
    /// * `value` - The value to append.
    ///
    /// # Returns
    ///
    /// * `Ok(())` if the element was successfully appended.
    /// * `Err(value)` if the `AllocVec` is at full capacity.
    ///
    /// # Time Complexity
    /// - *O*(1).
    ///
    #[inline]
    pub(crate) fn try_push(&mut self, value: T) -> Result<(), T> {
        if self.len == self.cap {
            return Err(value);
        }
        unsafe {
            ptr::write(self.ptr.as_ptr().add(self.len), value);
        }
        // Update length
        self.len += 1;
        Ok(())
    }

    /// Returns a reference to the element at the specified index, or `None` if out of bounds.
    ///
    /// # Arguments
    ///
    /// * `index` - The index of the element to retrieve.
    ///
    /// # Time Complexity
    /// - *O*(1).
    ///
    #[must_use]
    #[inline]
    pub(crate) fn get(&self, index: usize) -> Option<&T> {
        if index < self.len {
            unsafe { Some(&*self.ptr.as_ptr().add(index)) }
        } else {
            None
        }
    }

    /// Returns a reference to the element at the specified index, or `None` if out of bounds.
    ///
    /// # Arguments
    ///
    /// * `index` - The index of the element to retrieve.
    ///
    /// # Time Complexity
    /// - *O*(1).
    ///
    #[must_use]
    #[inline]
    pub(crate) fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        if index < self.len {
            unsafe { Some(&mut *self.ptr.as_ptr().add(index)) }
        } else {
            None
        }
    }

    /// Returns a reference to the first element.
    ///
    /// # Panics
    ///
    /// Panics if the `AllocVec` is empty.
    ///
    /// # Time Complexity
    /// - *O*(1).
    ///
    #[must_use]
    #[inline]
    pub(crate) fn first(&self) -> &T {
        assert!(self.len > 0, "Index out of bounds");
        unsafe { &*self.ptr.as_ptr() }
    }

    /// Returns a reference to the last element.
    ///
    /// # Panics
    ///
    /// Panics if the `AllocVec` is empty.
    ///
    /// # Time Complexity
    /// - *O*(1).
    ///
    #[must_use]
    #[inline]
    pub(crate) fn last(&self) -> &T {
        assert!(self.len > 0, "Index out of bounds");
        unsafe { &*self.ptr.as_ptr().add(self.len - 1) }
    }

    /// Removes and returns the element at the specified index.
    ///
    /// # Arguments
    ///
    /// * `index` - The index of the element to remove.
    ///
    /// # Panics
    ///
    /// Panics if the index is out of bounds.
    ///
    /// # Time Complexity
    /// -*O*(n) where n is the length of the `AllocVec`.
    ///
    pub(crate) fn remove(&mut self, index: usize) -> T {
        assert!(index < self.len, "Index out of bounds");
        // Update len first
        self.len -= 1;
        unsafe {
            let ptr = self.ptr.as_ptr().add(index);
            let value = ptr::read(ptr);
            // Shift everything to fill in.
            ptr::copy(ptr.add(1), ptr, self.len - index);
            value // ownership is transferred to the caller
        }
    }

    /// Removes the last element and returns it.
    ///
    /// # Panics
    ///
    /// Panics if the `AllocVec` is empty.
    ///
    /// # Time Complexity
    /// - *O*(1).
    ///
    #[inline]
    pub(crate) fn pop(&mut self) -> T {
        assert!(self.len > 0, "Index out of bounds");
        self.len -= 1;
        unsafe { ptr::read(self.ptr.as_ptr().add(self.len)) }
    }

    /// Removes the first element and returns it.
    ///
    /// # Panics
    ///
    /// Panics if the `AllocVec` is empty.
    ///
    /// # Time Complexity
    /// - *O*(n) where n is the length of the `AllocVec`.
    ///
    #[inline]
    pub(crate) fn pop_front(&mut self) -> T {
        assert!(self.len > 0, "Index out of bounds");
        let value = unsafe { ptr::read(self.ptr.as_ptr()) };
        self.len -= 1;
        unsafe {
            ptr::copy(self.ptr.as_ptr().add(1), self.ptr.as_ptr(), self.len);
        }
        value
    }

    /// Replaces the value at the given index with a new value.
    ///
    /// # Arguments
    ///
    /// * `index` - The index of the element to replace.
    /// * `new_value` - The new value to replace the old value with.
    ///
    /// # Panics
    ///
    /// Panics if the index is out of bounds.
    ///
    /// # Time Complexity
    /// - *O*(1).
    ///
    #[inline]
    pub(crate) fn replace(&mut self, index: usize, new_value: T) {
        assert!(index < self.len, "Index out of bounds");
        unsafe {
            let ptr = self.ptr.as_ptr().add(index);
            ptr::write(ptr, new_value);
        }
    }

    /// Swaps the values at the given indices.
    ///
    /// # Arguments
    ///
    /// * `index1` - The first index.
    /// * `index2` - The second index.
    ///
    /// # Panics
    ///
    /// Panics if either index is out of bounds.
    ///
    /// # Time Complexity
    /// - *O*(1).
    ///
    #[inline]
    pub(crate) fn swap(&mut self, index1: usize, index2: usize) {
        assert!(index1 < self.len, "Index1 out of bounds");
        assert!(index2 < self.len, "Index2 out of bounds");
        unsafe {
            let ptr1 = self.ptr.as_ptr().add(index1);
            let ptr2 = self.ptr.as_ptr().add(index2);
            ptr::swap(ptr1, ptr2);
        }
    }

    /// Returns an iterator over the chunks of the `AllocVec`.
    ///
    /// # Arguments
    ///
    /// * `chunk_size` - The size of each chunk.
    ///
    /// # Panics
    ///
    /// Panics if `chunk_size` is 0.
    ///
    /// # Time Complexity
    /// - *O*(1).
    ///
    #[inline]
    pub(crate) fn chunks(&self, chunk_size: usize) -> std::slice::Chunks<'_, T> {
        assert!(chunk_size > 0, "Chunk size must be greater than 0");
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.len).chunks(chunk_size) }
    }

    /// Returns an iterator over the mutable chunks of the `AllocVec`.
    ///
    /// # Arguments
    ///
    /// * `chunk_size` - The size of each chunk.
    ///
    /// # Panics
    ///
    /// Panics if `chunk_size` is 0.
    ///
    /// # Time Complexity
    /// - *O*(1).
    ///
    #[inline]
    pub(crate) fn chunks_mut(&mut self, chunk_size: usize) -> std::slice::ChunksMut<'_, T> {
        assert!(chunk_size > 0, "Chunk size must be greater than 0");
        unsafe {
            std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len).chunks_mut(chunk_size)
        }
    }

    /// Returns an iterator over the elements of the `AllocVec`.
    ///
    /// # Time Complexity
    /// - *O*(1).
    ///
    #[inline]
    pub(crate) fn iter(&self) -> std::slice::Iter<'_, T> {
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.len).iter() }
    }

    /// Returns a mutable iterator over the elements of the `AllocVec`.
    ///
    /// # Time Complexity
    /// - *O*(1).
    ///
    #[inline]
    pub(crate) fn iter_mut(&mut self) -> std::slice::IterMut<'_, T> {
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len).iter_mut() }
    }

    /// Clears the `AllocVec` and calls `drop` on elements.
    ///
    /// # Time Complexity
    /// - *O*(n) where n is the length of the `AllocVec`.
    ///
    #[inline]
    pub(crate) fn clear(&mut self) {
        if self.len != 0 {
            // Update len first
            self.len = 0;
            unsafe {
                // Call drop on each element to release resources.
                ptr::drop_in_place(std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len));
            }
        }
    }

    /// Returns the current memory usage of the `AllocVec` in bytes.
    #[inline]
    pub(crate) fn memory_usage(&self) -> usize {
        // Size of the metadata (cap and len)
        let metadata_size = size_of::<usize>() * 2;
        // Size of the allocated elements
        let elements_size = self.cap * size_of::<T>();
        // Total memory usage
        metadata_size + elements_size
    }
}

impl<T> Drop for AllocVec<T> {
    /// Drops the `AllocVec`, deallocating its memory.
    fn drop(&mut self) {
        if self.cap != 0 {
            let layout = Layout::array::<T>(self.cap).unwrap();
            unsafe {
                // Call drop on each element to release their resources.
                for i in 0..self.len {
                    ptr::drop_in_place(self.ptr.as_ptr().add(i));
                }
                // Deallocate memory space
                alloc::dealloc(self.ptr.as_ptr() as *mut u8, layout);
            }
        }
    }
}

impl<T> Default for AllocVec<T> {
    /// Returns the new `AllocVec` with a capacity of 0.
    #[must_use]
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Index<usize> for AllocVec<T> {
    type Output = T;

    /// Returns a reference to the element at the specified index.
    ///
    /// # Arguments
    ///
    /// * `index` - The index of the element to retrieve.
    ///
    /// # Panics
    ///
    /// Panics if the index is out of bounds.
    fn index(&self, index: usize) -> &Self::Output {
        assert!(index < self.len, "Index out of bounds");
        unsafe { &*self.ptr.as_ptr().add(index) }
    }
}

impl<T> IndexMut<usize> for AllocVec<T> {
    /// Returns a mutable reference to the element at the specified index.
    ///
    /// # Arguments
    ///
    /// * `index` - The index of the element to retrieve.
    ///
    /// # Panics
    ///
    /// Panics if the index is out of bounds.
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        assert!(index < self.len, "Index out of bounds");
        unsafe { &mut *self.ptr.as_ptr().add(index) }
    }
}

impl<T> Index<Range<usize>> for AllocVec<T> {
    type Output = [T];

    fn index(&self, range: Range<usize>) -> &Self::Output {
        assert!(
            range.start <= range.end,
            "Invalid range: start is greater than end"
        );
        assert!(range.end <= self.len, "Range out of bounds");
        unsafe {
            std::slice::from_raw_parts(self.ptr.as_ptr().add(range.start), range.end - range.start)
        }
    }
}

impl<T> IndexMut<Range<usize>> for AllocVec<T> {
    fn index_mut(&mut self, range: Range<usize>) -> &mut Self::Output {
        assert!(
            range.start <= range.end,
            "Invalid range: start is greater than end"
        );
        assert!(range.end <= self.len, "Range out of bounds");
        unsafe {
            std::slice::from_raw_parts_mut(
                self.ptr.as_ptr().add(range.start),
                range.end - range.start,
            )
        }
    }
}

impl<T: Default> AllocVec<T> {
    /// Creates a new `AllocVec` with the specified capacity and populates it with the default value of `T`.
    ///
    /// # Arguments
    ///
    /// * `cap` - The capacity of the new `AllocVec`.
    ///
    #[must_use]
    #[inline]
    pub(crate) fn with_capacity_and_populate(cap: usize) -> Self {
        if cap == 0 {
            return Self::new();
        }
        let layout = Layout::array::<T>(cap).expect("Allocation error: layout error");

        let ptr = unsafe { alloc::alloc(layout) as *mut T };

        let ptr = NonNull::new(ptr).expect("Allocation error: pointer is null");

        unsafe {
            for i in 0..cap {
                ptr::write(ptr.as_ptr().add(i), T::default());
            }
        }

        AllocVec {
            ptr,
            cap,
            len: cap,
            _marker: PhantomData,
        }
    }
}

impl<'a, T> IntoIterator for &'a AllocVec<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    /// Returns an iterator over the elements of the `AllocVec`.
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a, T> IntoIterator for &'a mut AllocVec<T> {
    type Item = &'a mut T;
    type IntoIter = std::slice::IterMut<'a, T>;

    /// Returns a mutable iterator over the elements of the `AllocVec`.
    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

/// An iterator over the elements of a `AllocVec`.
pub struct AllocVecIntoIter<T> {
    vec: AllocVec<T>,
    index: usize,
}

impl<T> Iterator for AllocVecIntoIter<T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.vec.len {
            unsafe {
                let item = ptr::read(self.vec.ptr.as_ptr().add(self.index));
                self.index += 1;
                Some(item)
            }
        } else {
            None
        }
    }
}

impl<T> IntoIterator for AllocVec<T> {
    type Item = T;
    type IntoIter = AllocVecIntoIter<T>;

    /// Consumes the `AllocVec` and returns an iterator over its elements.
    fn into_iter(self) -> Self::IntoIter {
        AllocVecIntoIter {
            vec: self,
            index: 0,
        }
    }
}

impl<T> FromIterator<T> for AllocVec<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let mut alloc_vec = AllocVec::new();
        for item in iter {
            alloc_vec.push(item);
        }
        alloc_vec
    }
}

impl<T> Deref for AllocVec<T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }
}

impl<T> DerefMut for AllocVec<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
    }
}

impl<T: Clone> Clone for AllocVec<T> {
    fn clone(&self) -> Self {
        let mut new_vec = AllocVec::with_capacity(self.cap);
        for i in 0..self.len {
            new_vec.push(self[i].clone());
        }
        new_vec
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alloc_vec_new() {
        let alloc_vec: AllocVec<i32> = AllocVec::new();
        assert_eq!(alloc_vec.capacity(), 0);
        assert_eq!(alloc_vec.len(), 0);
    }

    #[test]
    fn test_alloc_vec_with_capacity() {
        let alloc_vec: AllocVec<i32> = AllocVec::with_capacity(10);
        assert_eq!(alloc_vec.capacity(), 10);
        assert_eq!(alloc_vec.len(), 0);
    }

    #[test]
    fn test_alloc_vec_reserve() {
        let mut alloc_vec: AllocVec<i32> = AllocVec::with_capacity(10);
        assert_eq!(alloc_vec.capacity(), 10);
        alloc_vec.reserve(5);
        assert_eq!(alloc_vec.capacity(), 15);
    }

    #[test]
    fn test_alloc_vec_shrink() {
        let mut alloc_vec: AllocVec<i32> = AllocVec::with_capacity(10);
        alloc_vec.push(1);
        alloc_vec.push(2);
        alloc_vec.push(3);
        assert_eq!(alloc_vec.capacity(), 10);
        alloc_vec.shrink_to_fit();
        assert_eq!(alloc_vec.capacity(), 3);
        assert_eq!(alloc_vec.len(), 3);
        assert_eq!(alloc_vec[0], 1);
        assert_eq!(alloc_vec[1], 2);
        assert_eq!(alloc_vec[2], 3);
    }

    #[test]
    fn test_alloc_vec_push() {
        let mut alloc_vec: AllocVec<i32> = AllocVec::with_capacity(10);
        alloc_vec.push(1);
        assert!(!alloc_vec.is_empty());
        assert_eq!(alloc_vec.len(), 1);
    }

    #[test]
    fn test_try_push() {
        let mut alloc_vec: AllocVec<i32> = AllocVec::with_capacity(2);
        assert_eq!(alloc_vec.try_push(1), Ok(()));
        assert_eq!(alloc_vec.try_push(2), Ok(()));

        // Should return an error as capacity is full
        assert_eq!(alloc_vec.try_push(3), Err(3));
        assert_eq!(alloc_vec.len(), 2);
        assert_eq!(alloc_vec[0], 1);
        assert_eq!(alloc_vec[1], 2);
    }

    #[test]
    fn test_alloc_vec_get() {
        let mut alloc_vec: AllocVec<i32> = AllocVec::with_capacity(10);
        alloc_vec.push(1);
        alloc_vec.push(2);
        alloc_vec.push(3);
        assert_eq!(alloc_vec.get(0), Some(&1));
        assert_eq!(alloc_vec.get(1), Some(&2));
        assert_eq!(alloc_vec.get(2), Some(&3));
        assert_eq!(alloc_vec.get(3), None);
    }

    #[test]
    fn test_alloc_vec_get_mut() {
        let mut alloc_vec: AllocVec<i32> = AllocVec::with_capacity(10);
        alloc_vec.push(1);
        alloc_vec.push(2);
        alloc_vec.push(3);
        if let Some(value) = alloc_vec.get_mut(1) {
            *value = 10;
        }
        assert_eq!(alloc_vec.get(1), Some(&10));
    }

    #[test]
    fn test_alloc_vec_index() {
        let mut alloc_vec: AllocVec<i32> = AllocVec::with_capacity(10);
        alloc_vec.push(1);
        alloc_vec.push(2);
        assert_eq!(alloc_vec[0], 1);
        assert_eq!(alloc_vec[1], 2);
    }

    #[test]
    #[should_panic(expected = "Index out of bounds")]
    fn test_alloc_vec_index_out_of_bounds() {
        let mut alloc_vec: AllocVec<i32> = AllocVec::with_capacity(10);
        alloc_vec.push(10);

        let _ = alloc_vec[1];
    }

    #[test]
    fn test_alloc_vec_index_mut() {
        let mut alloc_vec: AllocVec<i32> = AllocVec::with_capacity(10);
        alloc_vec.push(1);
        alloc_vec[0] = 2;
        assert_eq!(alloc_vec[0], 2);
    }

    #[test]
    fn test_alloc_vec_first() {
        let mut alloc_vec: AllocVec<i32> = AllocVec::with_capacity(10);
        alloc_vec.push(1);
        alloc_vec.push(2);
        assert_eq!(alloc_vec.first(), &1);
    }

    #[test]
    #[should_panic(expected = "Index out of bounds")]
    fn test_alloc_vec_first_out_of_bounds() {
        let alloc_vec: AllocVec<i32> = AllocVec::with_capacity(10);
        let _ = alloc_vec.first();
    }

    #[test]
    fn test_alloc_vec_last() {
        let mut alloc_vec: AllocVec<i32> = AllocVec::with_capacity(10);
        alloc_vec.push(1);
        alloc_vec.push(2);
        assert_eq!(alloc_vec.last(), &2);
    }

    #[test]
    #[should_panic(expected = "Index out of bounds")]
    fn test_alloc_vec_last_out_of_bounds() {
        let alloc_vec: AllocVec<i32> = AllocVec::with_capacity(10);
        let _ = alloc_vec.last();
    }

    #[test]
    fn test_alloc_vec_pop_front() {
        let mut alloc_vec: AllocVec<i32> = AllocVec::with_capacity(10);
        alloc_vec.push(1);
        alloc_vec.push(2);
        alloc_vec.push(3);
        assert_eq!(alloc_vec.pop_front(), 1);
        assert_eq!(alloc_vec.len(), 2);
        assert_eq!(alloc_vec[0], 2);
        assert_eq!(alloc_vec[1], 3);
    }

    #[test]
    #[should_panic(expected = "Index out of bounds")]
    fn test_alloc_vec_pop_front_out_of_bounds() {
        let mut alloc_vec: AllocVec<i32> = AllocVec::with_capacity(10);
        alloc_vec.pop_front();
    }

    #[test]
    fn test_alloc_vec_pop() {
        let mut alloc_vec: AllocVec<i32> = AllocVec::with_capacity(10);
        alloc_vec.push(42);
        assert_eq!(alloc_vec.pop(), 42);
        assert_eq!(alloc_vec.len(), 0);
        assert!(alloc_vec.is_empty());
    }

    #[test]
    #[should_panic(expected = "Index out of bounds")]
    fn test_alloc_vec_pop_out_of_bounds() {
        let mut alloc_vec: AllocVec<i32> = AllocVec::with_capacity(10);
        alloc_vec.pop();
    }

    #[test]
    fn test_alloc_vec_remove() {
        let mut alloc_vec: AllocVec<i32> = AllocVec::with_capacity(10);
        alloc_vec.push(1);
        alloc_vec.push(2);
        assert_eq!(alloc_vec.remove(0), 1);
        assert_eq!(alloc_vec.len(), 1);
        assert_eq!(alloc_vec[0], 2);
    }

    #[test]
    #[should_panic(expected = "Index out of bounds")]
    fn test_alloc_vec_remove_out_of_bounds() {
        let mut alloc_vec: AllocVec<i32> = AllocVec::with_capacity(10);
        assert_eq!(alloc_vec.remove(0), 1);
    }

    #[test]
    fn test_alloc_vec_resize_with() {
        let mut alloc_vec: AllocVec<i32> = AllocVec::with_capacity(10);
        alloc_vec.resize_with(5, || 1);
        assert_eq!(alloc_vec.len(), 5);
        for i in 0..5 {
            assert_eq!(alloc_vec[i], 1);
        }
        alloc_vec.resize_with(2, || 10);
        assert_eq!(alloc_vec.len(), 2);
        assert_eq!(alloc_vec[0], 10);
        assert_eq!(alloc_vec[1], 10);
    }

    #[test]
    fn test_alloc_vec_swap() {
        let mut alloc_vec: AllocVec<i32> = AllocVec::with_capacity(3);
        alloc_vec.push(1);
        alloc_vec.push(2);
        alloc_vec.push(3);
        alloc_vec.swap(0, 2);
        assert_eq!(alloc_vec[0], 3);
        assert_eq!(alloc_vec[2], 1);
    }

    #[test]
    fn test_alloc_vec_replace() {
        let mut alloc_vec: AllocVec<i32> = AllocVec::with_capacity(3);
        alloc_vec.push(1);
        alloc_vec.push(2);
        alloc_vec.push(3);
        alloc_vec.replace(1, 10);
        assert_eq!(alloc_vec[1], 10);
    }

    #[test]
    fn test_alloc_vec_iter() {
        let mut alloc_vec: AllocVec<i32> = AllocVec::with_capacity(10);
        alloc_vec.push(1);
        alloc_vec.push(2);
        alloc_vec.push(3);
        let mut iter = alloc_vec.iter();
        assert_eq!(iter.next(), Some(&1));
        assert_eq!(iter.next(), Some(&2));
        assert_eq!(iter.next(), Some(&3));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn test_alloc_vec_iter_mut() {
        let mut alloc_vec: AllocVec<i32> = AllocVec::with_capacity(10);
        alloc_vec.push(1);
        alloc_vec.push(2);
        alloc_vec.push(3);
        for value in alloc_vec.iter_mut() {
            *value *= 2;
        }
        let mut iter = alloc_vec.iter();
        assert_eq!(iter.next(), Some(&2));
        assert_eq!(iter.next(), Some(&4));
        assert_eq!(iter.next(), Some(&6));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn test_alloc_vec_clear() {
        let mut alloc_vec: AllocVec<i32> = AllocVec::with_capacity(10);
        alloc_vec.push(1);
        alloc_vec.push(2);
        alloc_vec.push(3);
        alloc_vec.clear();
        assert_eq!(alloc_vec.len(), 0);
        assert!(alloc_vec.is_empty());
    }

    #[test]
    fn test_alloc_vec_for_loop() {
        let mut alloc_vec: AllocVec<i32> = AllocVec::with_capacity(10);
        alloc_vec.push(1);
        alloc_vec.push(2);
        alloc_vec.push(3);

        let mut sum = 0;
        for value in &alloc_vec {
            sum += *value;
        }
        assert_eq!(sum, 6);

        for value in &mut alloc_vec {
            *value *= 2;
        }

        let mut iter = alloc_vec.iter();
        assert_eq!(iter.next(), Some(&2));
        assert_eq!(iter.next(), Some(&4));
        assert_eq!(iter.next(), Some(&6));
        assert_eq!(iter.next(), None);

        let alloc_vec = alloc_vec;
        let mut iter = alloc_vec.into_iter();
        assert_eq!(iter.next(), Some(2));
        assert_eq!(iter.next(), Some(4));
        assert_eq!(iter.next(), Some(6));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn test_alloc_vec_deref_empty() {
        let alloc_vec: AllocVec<i32> = AllocVec::new();
        let slice: &[i32] = &*alloc_vec;
        assert_eq!(slice, &[]);
    }

    #[test]
    fn test_alloc_vec_deref() {
        let mut alloc_vec: AllocVec<i32> = AllocVec::with_capacity(10);
        alloc_vec.push(1);
        alloc_vec.push(2);
        alloc_vec.push(3);
        let slice: &[i32] = &*alloc_vec;
        assert_eq!(slice, &[1, 2, 3]);
    }

    #[test]
    fn test_alloc_vec_deref_mut_empty() {
        let mut alloc_vec: AllocVec<i32> = AllocVec::new();
        let slice: &mut [i32] = &mut *alloc_vec;
        assert_eq!(slice, &[]);
    }

    #[test]
    fn test_alloc_vec_deref_mut() {
        let mut alloc_vec: AllocVec<i32> = AllocVec::with_capacity(10);
        alloc_vec.push(1);
        alloc_vec.push(2);
        alloc_vec.push(3);
        let slice: &mut [i32] = &mut *alloc_vec;
        slice[0] = 10;
        assert_eq!(slice, &[10, 2, 3]);
    }

    #[test]
    fn test_alloc_vec_memory_usage() {
        let vec: AllocVec<i32> = AllocVec::with_capacity(10);
        let expected_memory_usage = size_of::<usize>() * 2 + 10 * size_of::<i32>();
        assert_eq!(vec.memory_usage(), expected_memory_usage);
    }

    #[test]
    fn test_alloc_vec_clone() {
        let mut original: AllocVec<i32> = AllocVec::with_capacity(10);
        original.push(1);
        original.push(2);
        original.push(3);

        let mut cloned = original.clone();

        // Cloned must have the same length and capacity
        assert_eq!(cloned.len(), original.len());
        assert_eq!(cloned.capacity(), original.capacity());

        // The elements in the clone must be the same as in the original
        for i in 0..original.len() {
            assert_eq!(cloned[i], original[i]);
        }

        // Mutating the clone must not affect the original
        cloned.push(4);
        assert_eq!(cloned.len(), original.len() + 1);
        assert_eq!(original.len(), 3); // original length
    }
}
