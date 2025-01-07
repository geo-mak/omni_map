use std::alloc::{self, alloc, Layout};
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut, Index, IndexMut, Range};
use std::ptr;
use std::fmt::Debug;

/// Debug-mode check for the layout size and alignment.
/// This function is only available in debug builds.
///
/// Conditions:
///
/// - `align` of `T` must not be zero.
///
/// - `align` of `T` must be a power of two.
///
/// - `size`, when rounded up to the nearest multiple of `align`, must be less than or
///   equal to `isize::MAX`.
///
#[cfg(debug_assertions)]
fn debug_layout_size_align(size: usize, align: usize) {
    // Alignment check
    assert!(align.is_power_of_two(), "Alignment must be a power of two");

    // Size check
    let max_size = (isize::MAX as usize + 1) - align;
    assert!(max_size > size , "Size exceeds maximum limit on this platform");
}

/// Debug-mode check to check the allocation state.
/// This function is only available in debug builds.
///
/// Conditions:
///
/// - The pointer must not be null.
///
/// - The capacity must not be `0`.
///
#[cfg(debug_assertions)]
fn debug_assert_allocated<T>(instance: &AllocVec<T>) {
    assert!(!instance.ptr.is_null(), "Pointer must not be null.");
    assert_ne!(instance.cap, 0, "Capacity must not be zero.");
}

/// Debug-mode check to check the allocation state.
/// This function is only available in debug builds.
///
/// Conditions:
///
/// - The pointer must be null.
///
/// - The capacity must be `0`.
///
#[cfg(debug_assertions)]
fn debug_assert_not_allocated<T>(instance: &AllocVec<T>) {
    assert!(instance.ptr.is_null(), "Pointer must be null.");
    assert_eq!(instance.cap, 0, "Capacity must be zero.");
}

/// Raw allocation buffer to enable better control over memory allocation.
///
/// This buffer uses the registered `global allocator` to allocate memory.
///
/// # Safety
///
/// The total size of the allocated memory when rounded up to the nearest multiple of `align`,
/// must be less than or equal to `isize::MAX`.
///
/// If the total size exceeds `isize::MAX` bytes, the memory allocation will fail.
///
/// # Internal representation:
///
/// - `ptr` is a non-null pointer to the first element of the vector.
/// - `len` is the number of elements in the vector.
/// - `cap` is the number of elements the vector can hold.
///
/// ```text
///            ptr  +  len   +  cap     --
///        *const T |  usize |  usize     |
///        +--------+--------+--------+   |
///        | 0x0123 |      2 |      4 |   |--> Metadata
///        +--------+--------+--------+   |
///             |                       --
///             v                                --
///        +--------+--------+--------+--------+   |
///        | val: T | val: T | uninit | uninit |   |--> Heap memory
///        +--------+--------+--------+--------+   |
///          0x0123   0x0127   0x012B   0x012F     |    Alignment is illustrative
///             0       1        2        3      --
///
/// ```
pub(crate) struct AllocVec<T> {
    ptr: *const T,
    cap: usize,
    len: usize,
    _marker: PhantomData<T>,
}

impl<T> AllocVec<T> {

    /// Creates a new, empty `AllocVec`.
    ///
    /// No memory is allocated, and the length and capacity are set to `0`.
    #[must_use]
    #[inline]
    pub(crate) const fn new() -> Self {
        // New dangling vector
        AllocVec {
            ptr: ptr::null(),
            cap: 0,
            len: 0,
            _marker: PhantomData,
        }
    }

    /// Creates a new `AllocVec` with the specified capacity.
    ///
    /// Memory is allocated for the specified capacity, and the length is set to 0.
    ///
    /// # Arguments
    ///
    /// - `cap` - The capacity of the new `AllocVec`.
    ///
    /// # Panics
    ///
    /// - When `cap` rounded up to the nearest multiple of `align` overflows `isize::MAX`.
    ///
    /// - When the allocator refuses to allocate memory space, this can happen when the system is
    ///   out of memory or the size of the requested block is too large.
    ///
    #[must_use]
    #[inline]
    pub(crate) fn new_allocate(cap: usize) -> Self {
        // New instance with no allocation
        let mut instance = Self::new();

        // No allocation required
        if cap == 0 {
            return instance;
        };

        // Allocate memory space
        instance.allocate(cap);

        // Return the new instance
        instance
    }

    /// Creates a new `AllocVec` with the specified capacity and populates it with the default
    /// value of `T`.
    ///
    /// Memory is allocated for the specified capacity, and the length is set to the capacity.
    ///
    /// # Arguments
    ///
    /// - `cap` - The capacity of the new `AllocVec`.
    ///
    /// # Panics
    ///
    /// - When `cap` rounded up to the nearest multiple of `align` overflows `isize::MAX`.
    ///
    /// - When the allocator refuses to allocate memory space, this can happen when the system is
    ///   out of memory or the size of the requested block is too large.
    ///
    #[must_use]
    #[inline]
    pub(crate) fn new_allocate_default(cap: usize) -> Self
    where T: Default
    {
        // New instance with no allocation
        let mut instance = Self::new();

        // No allocation required
        if cap == 0 {
            return instance;
        }

        // Allocate memory space
        instance.allocate(cap);

        // Set all elements to the default value of T
        instance.memset_default();

        // Return the new instance
        instance
    }

    /// Returns the capacity of the `AllocVec`.
    #[inline]
    pub(crate) const fn capacity(&self) -> usize {
        self.cap
    }

    /// Returns the number of initialized elements in the `AllocVec`.
    #[inline]
    pub(crate) const fn len(&self) -> usize {
        self.len
    }

    /// Returns `true` if the `AllocVec` is empty.
    #[inline]
    pub(crate) const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Allocates memory space for the `AllocVec`.
    ///
    /// # Safety
    ///
    /// - Pointer must be `null` and the current capacity must be `0`.
    ///   This method doesn't deallocate the old memory space pointed by the pointer.
    ///   Calling this method with a non-null pointer might cause memory leaks.
    ///   This condition is checked in debug mode only.
    ///
    /// - `cap` must be greater than `0`.
    ///   This condition is checked in debug mode only.
    ///
    /// - `cap`, when rounded up to the nearest multiple of `align`, must be less than or
    ///   equal to `isize::MAX`.
    ///   This condition is checked in debug mode only.
    ///
    pub(crate) fn allocate(&mut self, cap: usize) {
        // Pointer must be null and the current capacity must be 0
        #[cfg(debug_assertions)]
        debug_assert_not_allocated(self);

        // Not allowed to allocate zero capacity
        debug_assert_ne!(cap, 0, "Requested capacity must be greater than 0");

        // New layout
        let layout = unsafe {
            let layout_size = cap.unchecked_mul(size_of::<T>());

            // Debug-mode check for the layout size and alignment
            #[cfg(debug_assertions)]
            debug_layout_size_align(layout_size, align_of::<T>());

            Layout::from_size_align_unchecked(layout_size, align_of::<T>())
        };

        // Allocate memory space
        let ptr  = unsafe { alloc(layout) as *mut T };

        // Check if allocation failed
        if ptr.is_null() {
            // Allocation failed
            alloc::handle_alloc_error(layout);
        }

        // Update the pointer and capacity
        self.ptr = ptr;
        self.cap = cap;
    }

    /// Shrinks or grows the allocated memory space to the specified capacity.
    ///
    /// # Safety
    ///
    /// - Pointer must be allocated and the current capacity must be greater than `0`.
    ///   This condition is checked in debug mode only.
    ///
    /// - `new_cap`, when rounded up to the nearest multiple of `align`, must be less than or
    ///   equal to `isize::MAX`.
    ///   This condition is checked in debug mode only.
    ///
    /// - `new_cap` must be greater than or equal to the current length.
    ///   Reallocating capacity less than the current length might cause memory leaks, as the
    ///   elements will be out of bounds without being dropped properly.
    ///   This condition is checked in debug mode only.
    ///
    pub(crate) fn reallocate(&mut self, new_cap: usize) {
        #[cfg(debug_assertions)]
        debug_assert_allocated(self);

        // Reallocating capacity less than the current length is not allowed.
        debug_assert!(
            new_cap >= self.len,
            "New capacity must be greater than or equal to the current length."
        );

        let t_size = size_of::<T>(); // Size of T, const
        let t_align = align_of::<T>(); // Alignment of T, const

        // New size
        let new_size = unsafe {
            new_cap.unchecked_mul(t_size)
        };

        // Debug-mode check for the new layout
        #[cfg(debug_assertions)]
        debug_layout_size_align(new_size, t_align);

        // Current layout
        let current_layout = unsafe {
            // Already checked in the `allocate_layout` function
            let current_size = self.cap.unchecked_mul(t_size);
            Layout::from_size_align_unchecked(current_size, t_align)
        };

        // Reallocate memory space
        let new_ptr = unsafe {
            alloc::realloc(self.ptr as *mut u8, current_layout, new_size) as *mut T
        };

        // Check if reallocation failed
        if new_ptr.is_null() {
            // Reallocate failed
            alloc::handle_alloc_error(current_layout);
        }

        // Update the pointer and capacity
        self.ptr = new_ptr;
        self.cap = new_cap;
    }

    /// Sets all elements in the allocated memory space to the default value of `T`.
    /// The length will be updated to the current capacity.
    ///
    /// If no memory is allocated, this method will do nothing.
    ///
    /// # Safety
    ///
    /// Initialized elements will be overwritten **without** calling `drop`.
    /// This might cause memory leaks if the elements are not of trivial type,
    /// or not dropped properly.
    ///
    /// # Time Complexity
    ///
    /// _O_(n) where n is current capacity of the `AllocVec`.
    ///
    #[inline]
    pub(crate) fn memset_default(&mut self)
    where T: Default
    {
        // Write the value to all elements
        unsafe {
            for i in 0..self.cap {
                ptr::write((self.ptr as *mut T).add(i), T::default());
            }
        }

        // Update length
        self.len = self.cap;
    }

    /// Stores a value after the last initialized element.
    ///
    /// # Safety
    ///
    /// This method will **not** grow the capacity automatically.
    ///
    /// The caller must ensure that the `AllocVec` has enough capacity to hold the new element.
    ///
    /// Calling this method without enough capacity will cause termination with `SIGSEGV`.
    ///
    /// This condition is checked in debug mode only.
    ///
    /// # Arguments
    ///
    /// - `value` - The value to append.
    ///
    /// # Time Complexity
    ///
    /// _O_(1).
    ///
    #[inline]
    pub(crate) fn store_next(&mut self, value: T) {
        // This must be ensured by the caller.
        #[cfg(debug_assertions)]
        debug_assert_allocated(self);
        debug_assert!(self.len < self.cap, "Capacity overflow.");

        unsafe {
            ptr::write((self.ptr as *mut T).add(self.len), value);
        }
        // Update length
        self.len += 1;
    }

    /// Returns a reference to the first initialized element.
    ///
    ///
    /// # Safety
    ///
    /// This method checks for out of bounds access in debug mode only.
    ///
    /// The caller must ensure that the `AllocVec` is not empty.
    ///
    /// # Time Complexity
    ///
    /// _O_(1).
    ///
    #[must_use]
    #[inline]
    pub(crate) fn load_first(&self) -> &T {
        // This must be ensured by the caller.
        debug_assert!(self.len > 0, "Index out of bounds");
        unsafe { &*self.ptr }
    }

    /// Returns a reference to the last initialized element.
    ///
    /// # Safety
    ///
    /// This method checks for out of bounds access in debug mode only.
    ///
    /// The caller must ensure that the `AllocVec` is not empty.
    ///
    /// # Time Complexity
    ///
    /// _O_(1).
    ///
    #[must_use]
    #[inline]
    pub(crate) fn load_last(&self) -> &T {
        // This must be ensured by the caller.
        debug_assert!(self.len > 0, "Index out of bounds");
        unsafe { &*self.ptr.add(self.len - 1) }
    }

    /// Removes and returns the initialized element at the specified index.
    ///
    /// # Arguments
    ///
    /// - `index` - The index of the element to remove.
    ///
    /// # Safety
    ///
    /// This method checks for out of bounds access in debug mode only.
    ///
    /// The caller must ensure that `index` is within the bounds of the initialized elements.
    ///
    /// # Time Complexity
    ///
    /// _O_(n) where n is the length of the `AllocVec` minus the index.
    ///
    pub(crate) fn take(&mut self, index: usize) -> T {
        // This must be ensured by the caller.
        debug_assert!(index < self.len, "Index out of bounds");
        unsafe {
            // infallible
            let value;
            {
                // The source offset
                let src = (self.ptr as *mut T).add(index);

                // The destination offset
                let dst = src.add(1);

                // Copy value to the stack
                value = ptr::read(src);

                // Shift everything down to fill in.
                ptr::copy(dst, src, self.len - index - 1);
            }

            // Update len
            self.len -= 1;

            // Ownership is transferred to the caller
            value
        }
    }

    /// Removes the last initialized element and returns it.
    ///
    /// # Safety
    ///
    /// This method checks for out of bounds access in debug mode only.
    ///
    /// The caller must ensure that the number of initialized elements is greater than `0`.
    ///
    /// # Time Complexity
    ///
    /// _O_(1).
    ///
    #[inline]
    pub(crate) fn take_last(&mut self) -> T {
        // This must be ensured by the caller.
        debug_assert!(self.len > 0, "Index out of bounds");
        self.len -= 1;
        unsafe { ptr::read(self.ptr.add(self.len)) }
    }

    /// Removes the first initialized element and returns it.
    ///
    /// # Safety
    ///
    /// This method checks for out of bounds access in debug mode only.
    ///
    /// The caller must ensure that the number of initialized elements is greater than `0`.
    ///
    /// # Time Complexity
    ///
    /// _O_(n) where n is the length of the `AllocVec` minus 1.
    ///
    #[inline]
    pub(crate) fn take_first(&mut self) -> T {
        // This must be ensured by the caller.
        debug_assert!(self.len > 0, "Index out of bounds");
        unsafe {
            // infallible
            let value;
            {
                // The old start offset
                let src = self.ptr as *mut T;

                // The new start offset
                let dst = src.add(1);

                // Copy value to the stack
                value = ptr::read(src);

                // Shift everything down to fill in.
                ptr::copy(dst, src, self.len - 1);
            }

            // Update len
            self.len -= 1;

            // Ownership is transferred to the caller
            value
        }
    }

    /// Calls `drop` on all initialized elements and sets the length to `0`.
    /// If there are no initialized elements, this method will do nothing.
    ///
    /// # Safety
    ///
    /// Pointer must be allocated and the current capacity must be greater than `0`.
    /// This condition is checked in debug mode only.
    ///
    /// # Time Complexity
    ///
    /// _O_(n) where n is the length of the `AllocVec`.
    ///
    #[inline]
    pub(crate) fn drop_init(&mut self) {
        #[cfg(debug_assertions)]
        debug_assert_allocated(self);

        // Update len first
        self.len = 0;
        unsafe {
            // Call drop on each element to release resources.
            ptr::drop_in_place(std::slice::from_raw_parts_mut(self.ptr as *mut T, self.len));
        }
    }
    
    /// Replaces the value at the given index with a new value and returns the old value.
    ///
    /// # Arguments
    ///
    /// - `index` - The index of the element to replace.
    ///
    /// - `new_value` - The new value to replace the old value with.
    ///
    /// # Panics
    ///
    /// Panics if the index is out of bounds of the initialized elements.
    ///
    /// # Time Complexity
    ///
    /// _O_(1).
    ///
    #[inline]
    pub(crate) fn replace(&mut self, index: usize, new_value: T) -> T {
        // This must be release-mode check because the exposing API is expected to be the same.
        assert!(index < self.len, "Index out of bounds");
        unsafe {
            let ptr = (self.ptr as *mut T).add(index);
            ptr::replace(ptr, new_value)
        }
    }

    /// Swaps the values at the given indices.
    ///
    /// # Arguments
    ///
    /// - `index1` - The first index.
    ///
    /// - `index2` - The second index.
    ///
    /// # Panics
    ///
    /// Panics if either index is out of bounds of the initialized elements.
    ///
    /// # Time Complexity
    ///
    /// _O_(1).
    ///
    #[inline]
    pub(crate) fn swap(&mut self, index1: usize, index2: usize) {
        // This must be release-mode check because the exposing API is expected to be the same.
        assert!(index1 < self.len, "Index1 out of bounds");
        assert!(index2 < self.len, "Index2 out of bounds");
        unsafe {
            let ptr1 = (self.ptr as *mut T).add(index1);
            let ptr2 = (self.ptr as *mut T).add(index2);
            ptr::swap(ptr1, ptr2);
        }
    }

    /// Returns an iterator over the chunks of the `AllocVec`.
    ///
    /// # Arguments
    ///
    /// - `chunk_size` - The size of each chunk.
    ///
    /// # Panics
    ///
    /// Panics if `chunk_size` is 0.
    ///
    /// # Time Complexity
    ///
    /// _O_(1).
    ///
    #[inline]
    pub(crate) fn chunks(&self, chunk_size: usize) -> std::slice::Chunks<'_, T> {
        // This must be release-mode check because the exposing API is expected to be the same.
        assert!(chunk_size > 0, "Chunk size must be greater than 0");
        unsafe { std::slice::from_raw_parts(self.ptr, self.len).chunks(chunk_size) }
    }

    /// Returns an iterator over the mutable chunks of the `AllocVec`.
    ///
    /// # Arguments
    ///
    /// - `chunk_size` - The size of each chunk.
    ///
    /// # Panics
    ///
    /// Panics if `chunk_size` is 0.
    ///
    /// # Time Complexity
    ///
    /// _O_(1).
    ///
    #[inline]
    pub(crate) fn chunks_mut(&mut self, chunk_size: usize) -> std::slice::ChunksMut<'_, T> {
        // This must be release-mode check because the exposing API is expected to be the same.
        assert!(chunk_size > 0, "Chunk size must be greater than 0");
        unsafe {
            std::slice::from_raw_parts_mut(self.ptr as *mut T, self.len).chunks_mut(chunk_size)
        }
    }

    /// Returns an iterator over the elements of the `AllocVec`.
    /// If the `AllocVec` is empty, the iterator will return an empty slice.
    ///
    /// # Safety
    ///
    /// This method is safe to call even if the pointer is null.
    ///
    /// # Time Complexity
    ///
    /// _O_(1).
    ///
    #[inline]
    pub(crate) fn iter(&self) -> std::slice::Iter<'_, T> {
        if self.len == 0 {
            [].iter()
        } else {
            unsafe { std::slice::from_raw_parts(self.ptr, self.len).iter() }
        }
    }

    /// Returns a mutable iterator over the initialized elements of the `AllocVec`.
    /// If the `AllocVec` is empty, the iterator will return an empty slice.
    ///
    /// # Safety
    ///
    /// This method is safe to call even if the pointer is null.
    ///
    /// # Time Complexity
    ///
    /// _O_(1).
    ///
    #[inline]
    pub(crate) fn iter_mut(&mut self) -> std::slice::IterMut<'_, T> {
        if self.len == 0 {
            [].iter_mut()
        } else {
            unsafe { std::slice::from_raw_parts_mut(self.ptr as *mut T, self.len).iter_mut() }
        }
    }
    
    /// Returns the current memory usage of the `AllocVec` in bytes.
    ///
    /// The result is sum of the size of the metadata (ptr, cap and len) and the size of the
    /// allocated elements.
    ///
    /// > Note:
    /// > The result is only an approximation of the memory usage.
    /// > For example, if `T` is `Box<A>`, the memory usage of `A` will not be included.
    ///
    #[inline]
    pub(crate) fn memory_usage(&self) -> usize {
        // Size of the metadata (ptr, cap and len)
        let metadata_size = size_of::<usize>() * 3;
        // Size of the allocated elements
        let elements_size = self.cap * size_of::<T>();
        // Total memory usage
        metadata_size + elements_size
    }
}

impl<T> Drop for AllocVec<T> {
    /// Calls drop on each element and deallocates the memory space.
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe {
                // Already checked in the `allocate_layout` function.
                // `size_of` and `align_of` are const.
                let layout = Layout::from_size_align_unchecked(
                    self.cap * size_of::<T>(),
                    align_of::<T>()
                );
                // Call drop on each element to release their resources.
                ptr::drop_in_place(std::slice::from_raw_parts_mut(self.ptr as *mut T, self.len));
                // Deallocate memory space.
                alloc::dealloc(self.ptr as *mut u8, layout);
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
    /// - `index` - The index of the element to retrieve.
    ///
    /// # Panics
    ///
    /// Panics if the index is out of bounds.
    ///
    fn index(&self, index: usize) -> &Self::Output {
        // This must be release-mode check because the exposing API is expected to be the same.
        assert!(index < self.len, "Index out of bounds");
        unsafe { &*self.ptr.add(index) }
    }
}

impl<T> IndexMut<usize> for AllocVec<T> {
    /// Returns a mutable reference to the element at the specified index.
    ///
    /// # Arguments
    ///
    /// - `index` - The index of the element to retrieve.
    ///
    /// # Panics
    ///
    /// Panics if the index is out of bounds.
    ///
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        // This must be release-mode check because the exposing API is expected to be the same.
        assert!(index < self.len, "Index out of bounds");
        unsafe { &mut *(self.ptr as *mut T).add(index) }
    }
}

impl<T> Index<Range<usize>> for AllocVec<T> {
    type Output = [T];

    fn index(&self, range: Range<usize>) -> &Self::Output {
        // These must be release-mode checks because the exposing API is expected to be the same.
        assert!(
            range.start <= range.end,
            "Invalid range: start is greater than end"
        );
        assert!(range.end <= self.len, "Range is out of bounds");
        unsafe {
            std::slice::from_raw_parts(self.ptr.add(range.start), range.end - range.start)
        }
    }
}

impl<T> IndexMut<Range<usize>> for AllocVec<T> {
    fn index_mut(&mut self, range: Range<usize>) -> &mut Self::Output {
        // This must be release-mode check because the exposing API is expected to be the same.
        assert!(
            range.start <= range.end,
            "Invalid range: start is greater than end"
        );
        assert!(range.end <= self.len, "Range out of bounds");
        unsafe {
            std::slice::from_raw_parts_mut(
                (self.ptr as *mut T).add(range.start),
                range.end - range.start,
            )
        }
    }
}

impl<'a, T> IntoIterator for &'a AllocVec<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    /// Returns an iterator over the initialized elements of the `AllocVec`.
    fn into_iter(self) -> Self::IntoIter {
        // This call is safe even if the pointer is null.
        self.iter()
    }
}

impl<'a, T> IntoIterator for &'a mut AllocVec<T> {
    type Item = &'a mut T;
    type IntoIter = std::slice::IterMut<'a, T>;

    /// Returns a mutable iterator over the initialized elements of the `AllocVec`.
    fn into_iter(self) -> Self::IntoIter {
        // This call is safe even if the pointer is null.
        self.iter_mut()
    }
}

/// An iterator over the initialized elements of the `AllocVec`.
pub(crate) struct AllocVecIntoIter<T> {
    vec: AllocVec<T>,
    index: usize,
}

impl<T> Iterator for AllocVecIntoIter<T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        // If len > 0, then the pointer is not null.
        if self.index < self.vec.len {
            unsafe {
                let item = ptr::read(self.vec.ptr.add(self.index));
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

    /// Consumes the `AllocVec` and returns an iterator over its initialized elements.
    fn into_iter(self) -> Self::IntoIter {
        AllocVecIntoIter {
            vec: self,
            index: 0,
        }
    }
}

impl<T> Deref for AllocVec<T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        if self.len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
        }
    }
}

impl<T> DerefMut for AllocVec<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        if self.len == 0 {
            &mut []
        } else {
            unsafe { std::slice::from_raw_parts_mut(self.ptr as *mut T, self.len) }
        }
    }
}

impl<T: PartialEq> PartialEq for AllocVec<T> {
    fn eq(&self, other: &Self) -> bool {
        if self.len != other.len {
            return false;
        }
        self.iter().zip(other.iter()).all(|(a, b)| a.eq(b))
    }
}

impl<T: Clone> AllocVec<T> {
    /// Clones the `AllocVec` with two possible modes: `compact` or `full`.
    fn clone_in(&self, compact: bool) -> Self {
        // New dangling vector
        let mut new_vec = AllocVec {
            ptr: ptr::null(),
            cap: 0,
            len: 0,
            _marker: PhantomData,
        };

        // No allocation required either way
        if self.cap == 0 || (compact && self.len == 0) {
            return new_vec;
        }

        // cap here must be greater than 0 either way (self.cap or self.len)
        let cap = if compact { self.len } else { self.cap };

        // Allocate memory space
        new_vec.allocate(cap);

        // Clone elements
        unsafe {
            let src_slice = std::slice::from_raw_parts(self.ptr, self.len);
            let dest_slice = std::slice::from_raw_parts_mut(new_vec.ptr as *mut T, self.len);
            dest_slice.clone_from_slice(src_slice);
        }

        // Set the new length
        new_vec.len = self.len;

        // Clone is complete
        new_vec
    }

    /// Clones the `AllocVec` with capacity equal to the length.
    #[must_use]
    pub(crate) fn clone_compact(&self) -> Self {
        self.clone_in(true)
    }
}

impl<T: Clone> Clone for AllocVec<T> {
    fn clone(&self) -> Self {
        self.clone_in(false)
    }
}

impl<T: Debug> Debug for AllocVec<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alloc_vec_new() {
        let alloc_vec: AllocVec<u8> = AllocVec::new();

        assert!(alloc_vec.ptr.is_null());
        assert_eq!(alloc_vec.capacity(), 0);
        assert_eq!(alloc_vec.len(), 0);
    }

    #[test]
    fn test_alloc_vec_new_allocate() {
        let alloc_vec: AllocVec<u8> = AllocVec::new_allocate(10);

        assert!(!alloc_vec.ptr.is_null());
        assert_eq!(alloc_vec.capacity(), 10);
        assert_eq!(alloc_vec.len(), 0);
    }

    #[test]
    #[cfg(debug_assertions)]
    fn test_alloc_vec_new_allocate_zero_cap() {
        let alloc_vec: AllocVec<u8> = AllocVec::new_allocate(0);

        // Capacity is 0, no allocation should have been made
        assert!(alloc_vec.ptr.is_null());
        assert_eq!(alloc_vec.capacity(), 0);
        assert_eq!(alloc_vec.len(), 0);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Size exceeds maximum limit on this platform")]
    fn test_alloc_vec_new_allocate_overflow() {
        let _: AllocVec<u8> = AllocVec::new_allocate(isize::MAX as usize + 1);
    }

    #[test]
    fn test_alloc_vec_allocate() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::new();

        // Allocate memory space
        alloc_vec.allocate(10);

        assert!(!alloc_vec.ptr.is_null());
        assert_eq!(alloc_vec.capacity(), 10);
        assert_eq!(alloc_vec.len(), 0);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Requested capacity must be greater than 0")]
    fn test_alloc_vec_allocate_zero_cap() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::new();

        // Capacity must be greater than 0, should panic
        alloc_vec.allocate(0);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Size exceeds maximum limit on this platform")]
    fn test_alloc_vec_allocate_overflow() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::new();

        // Size exceeds maximum limit, should panic
        alloc_vec.allocate(isize::MAX as usize + 1);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Pointer must be null.")]
    fn test_alloc_vec_allocate_allocated() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::new();

        // Not yet allocated, should not panic
        alloc_vec.allocate(1);

        assert!(!alloc_vec.ptr.is_null());
        assert_eq!(alloc_vec.capacity(), 1);

        // Already allocated, should panic
        alloc_vec.allocate(2);
    }

    #[allow(dead_code)]
    enum Choice {
        Custom,
        Default,
    }

    impl Default for Choice {
        fn default() -> Self {
            Choice::Default
        }
    }

    #[test]
    fn test_alloc_vec_memset_default() {
        let mut alloc_vec: AllocVec<Choice> = AllocVec::new_allocate(10);
        assert_eq!(alloc_vec.capacity(), 10);
        assert_eq!(alloc_vec.len(), 0);

        // Set all elements to the default value of `Choice`
        alloc_vec.memset_default();

        // Len was 0, so it should be updated to 10
        assert_eq!(alloc_vec.len(), 10);

        // Values were uninit, so they should be set to `Default`
        for i in 0..10 {
            assert!(matches!(alloc_vec[i], Choice::Default))
        }
    }

    #[test]
    fn test_alloc_vec_new_allocate_default() {
        let capacity = 5;
        let alloc_vec: AllocVec<Choice> = AllocVec::new_allocate_default(capacity);

        // Memory space should have been allocated
        assert!(!alloc_vec.ptr.is_null());
        assert_eq!(alloc_vec.capacity(), capacity);
        assert_eq!(alloc_vec.len(), capacity);

        // All elements are must have been initialized to their default values
        for i in 0..capacity {
            assert!(matches!(alloc_vec[i], Choice::Default))
        }
    }

    #[test]
    fn test_alloc_vec_new_allocate_default_zero_cap() {
        let alloc_vec: AllocVec<u8> = AllocVec::new_allocate_default(0);

        // Capacity is 0, no allocation should have been made
        assert!(alloc_vec.ptr.is_null());
        assert_eq!(alloc_vec.capacity(), 0);
        assert_eq!(alloc_vec.len(), 0);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Size exceeds maximum limit on this platform")]
    fn test_alloc_vec_new_allocate_default_overflow() {
        let _: AllocVec<u8> = AllocVec::new_allocate_default(isize::MAX as usize + 1);
    }

    #[test]
    fn test_alloc_vec_reallocate() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::new_allocate(3);
        assert_eq!(alloc_vec.capacity(), 3);

        alloc_vec.store_next(1);
        alloc_vec.store_next(2);
        alloc_vec.store_next(3);

        assert_eq!(alloc_vec.len(), 3);

        // Grows the capacity to 5
        alloc_vec.reallocate(5);

        assert_eq!(alloc_vec.capacity(), 5);

        // Check values after reallocation
        for i in 0..3 {
            assert_eq!(alloc_vec[i], i as u8 + 1);
        }
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Pointer must not be null.")]
    fn test_alloc_vec_reallocate_null_ptr() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::new();

        // Not yet allocated, should panic
        alloc_vec.reallocate(10);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "New capacity must be greater than or equal to the current length")]
    fn test_alloc_vec_reallocate_less_than_len() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::new_allocate(3);
        alloc_vec.store_next(1);
        alloc_vec.store_next(2);
        alloc_vec.store_next(3);

        // New capacity is less than the current length, should panic
        alloc_vec.reallocate(2);
    }

    #[test]
    fn test_alloc_vec_push() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::new_allocate(10);
        alloc_vec.store_next(2);
        assert_eq!(alloc_vec.len(), 1);

        let pushed_value = unsafe { *alloc_vec.ptr };

        assert_eq!(pushed_value, 2);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Pointer must not be null.")]
    fn test_alloc_vec_push_overflow() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::new();

        // Not yet allocated, should panic
        alloc_vec.store_next(1);
    }

    #[test]
    fn test_alloc_vec_index() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::new_allocate(10);
        alloc_vec.store_next(1);
        alloc_vec.store_next(2);
        assert_eq!(alloc_vec[0], 1);
        assert_eq!(alloc_vec[1], 2);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Index out of bounds")]
    fn test_alloc_vec_index_out_of_bounds() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::new_allocate(10);
        alloc_vec.store_next(10);

        // Index out of bounds, should panic
        let _ = alloc_vec[1];
    }

    #[test]
    fn test_alloc_vec_index_mut() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::new_allocate(10);
        alloc_vec.store_next(1);
        alloc_vec[0] = 2;
        assert_eq!(alloc_vec[0], 2);
    }

    #[test]
    fn test_alloc_vec_index_range() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::new_allocate(10);
        alloc_vec.store_next(1);
        alloc_vec.store_next(2);
        alloc_vec.store_next(3);
        alloc_vec.store_next(4);

        // Read values in the range [1, 3)
        let slice = &alloc_vec[1..3];

        // Verify the values
        assert_eq!(slice, &[2, 3]);
    }

    #[test]
    fn test_alloc_vec_index_range_mut() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::new_allocate(10);
        alloc_vec.store_next(1);
        alloc_vec.store_next(2);
        alloc_vec.store_next(3);
        alloc_vec.store_next(4);

        // Mutate values in the range [1, 4)
        for value in &mut alloc_vec[1..3] {
            *value *= 2;
        }

        // Verify the changes
        assert_eq!(alloc_vec[0], 1);
        assert_eq!(alloc_vec[1], 4);
        assert_eq!(alloc_vec[2], 6);

        // Verify the rest of the values
        assert_eq!(alloc_vec[3], 4);
    }

    #[test]
    fn test_alloc_vec_first() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::new_allocate(10);
        alloc_vec.store_next(1);
        alloc_vec.store_next(2);
        assert_eq!(alloc_vec.load_first(), &1);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Index out of bounds")]
    fn test_alloc_vec_first_out_of_bounds() {
        let alloc_vec: AllocVec<u8> = AllocVec::new_allocate(10);
        let _ = alloc_vec.load_first();
    }

    #[test]
    fn test_alloc_vec_last() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::new_allocate(10);
        alloc_vec.store_next(1);
        alloc_vec.store_next(2);
        assert_eq!(alloc_vec.load_last(), &2);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Index out of bounds")]
    fn test_alloc_vec_last_out_of_bounds() {
        let alloc_vec: AllocVec<u8> = AllocVec::new_allocate(10);
        let _ = alloc_vec.load_last();
    }

    #[test]
    fn test_alloc_vec_pop_front() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::new_allocate(10);
        alloc_vec.store_next(1);
        alloc_vec.store_next(2);
        alloc_vec.store_next(3);
        assert_eq!(alloc_vec.take_first(), 1);
        assert_eq!(alloc_vec.len(), 2);
        assert_eq!(alloc_vec[0], 2);
        assert_eq!(alloc_vec[1], 3);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Index out of bounds")]
    fn test_alloc_vec_pop_front_out_of_bounds() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::new_allocate(10);
        alloc_vec.take_first();
    }

    #[test]
    fn test_alloc_vec_pop() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::new_allocate(10);
        alloc_vec.store_next(42);
        assert_eq!(alloc_vec.take_last(), 42);
        assert_eq!(alloc_vec.len(), 0);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Index out of bounds")]
    fn test_alloc_vec_pop_out_of_bounds() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::new_allocate(10);
        alloc_vec.take_last();
    }

    #[test]
    fn test_alloc_vec_remove() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::new_allocate(10);
        alloc_vec.store_next(1);
        alloc_vec.store_next(2);
        assert_eq!(alloc_vec.take(0), 1);
        assert_eq!(alloc_vec.len(), 1);
        assert_eq!(alloc_vec[0], 2);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Index out of bounds")]
    fn test_alloc_vec_remove_out_of_bounds() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::new_allocate(10);
        assert_eq!(alloc_vec.take(0), 1);
    }

    #[test]
    fn test_alloc_vec_swap() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::new_allocate(3);
        alloc_vec.store_next(1);
        alloc_vec.store_next(2);
        alloc_vec.store_next(3);
        alloc_vec.swap(0, 2);
        assert_eq!(alloc_vec[0], 3);
        assert_eq!(alloc_vec[2], 1);
    }

    #[test]
    fn test_alloc_vec_replace() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::new_allocate(3);
        alloc_vec.store_next(1);
        alloc_vec.store_next(2);
        alloc_vec.store_next(3);
        let old_value = alloc_vec.replace(1, 10);
        assert_eq!(alloc_vec[1], 10);
        assert_eq!(old_value, 2);
    }

    #[test]
    fn test_alloc_vec_iter() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::new_allocate(10);
        alloc_vec.store_next(1);
        alloc_vec.store_next(2);
        alloc_vec.store_next(3);
        let mut iter = alloc_vec.iter();
        assert_eq!(iter.next(), Some(&1));
        assert_eq!(iter.next(), Some(&2));
        assert_eq!(iter.next(), Some(&3));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn test_alloc_vec_iter_mut() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::new_allocate(10);
        alloc_vec.store_next(1);
        alloc_vec.store_next(2);
        alloc_vec.store_next(3);
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
    fn test_alloc_vec_for_loop() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::new_allocate(3);
        alloc_vec.store_next(1);
        alloc_vec.store_next(2);
        alloc_vec.store_next(3);
        let mut sum = 0;
        // Immutable borrow
        for value in &alloc_vec {
            sum += *value;
        }
        assert_eq!(sum, 6);
    }

    #[test]
    fn test_alloc_vec_for_loop_mut() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::new_allocate(3);
        alloc_vec.store_next(1);
        alloc_vec.store_next(2);
        alloc_vec.store_next(3);
        // Mutable borrow
        for value in &mut alloc_vec {
            *value *= 2;
        }
        let mut iter = alloc_vec.iter();
        assert_eq!(iter.next(), Some(&2));
        assert_eq!(iter.next(), Some(&4));
        assert_eq!(iter.next(), Some(&6));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn test_alloc_vec_into_iterator(){
        let mut alloc_vec: AllocVec<u8> = AllocVec::new_allocate(3);
        alloc_vec.store_next(1);
        alloc_vec.store_next(2);
        alloc_vec.store_next(3);
        let mut iter: AllocVecIntoIter<u8> = alloc_vec.into_iter();
        assert_eq!(iter.next(), Some(1));
        assert_eq!(iter.next(), Some(2));
        assert_eq!(iter.next(), Some(3));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn test_alloc_vec_deref_empty() {
        let alloc_vec: AllocVec<u8> = AllocVec::new();
        let slice: &[u8] = &*alloc_vec;
        assert!(slice.is_empty());
    }

    #[test]
    fn test_alloc_vec_deref() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::new_allocate(10);
        alloc_vec.store_next(1);
        alloc_vec.store_next(2);
        alloc_vec.store_next(3);
        let slice: &[u8] = &*alloc_vec;
        assert_eq!(slice, &[1, 2, 3]);
    }

    #[test]
    fn test_alloc_vec_deref_mut_empty() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::new();
        let slice: &mut [u8] = &mut *alloc_vec;
        assert!(slice.is_empty());
    }

    #[test]
    fn test_alloc_vec_deref_mut() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::new_allocate(10);
        alloc_vec.store_next(1);
        alloc_vec.store_next(2);
        alloc_vec.store_next(3);
        let slice: &mut [u8] = &mut *alloc_vec;
        slice[0] = 10;
        assert_eq!(slice, &[10, 2, 3]);
    }

    #[test]
    fn test_alloc_vec_clear() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::new_allocate(10);
        alloc_vec.store_next(1);
        alloc_vec.store_next(2);
        alloc_vec.store_next(3);
        alloc_vec.drop_init();
        assert_eq!(alloc_vec.len(), 0);
    }

    #[test]
    fn test_alloc_vec_drop() {
        use std::rc::Rc;
        use std::cell::RefCell;

        #[derive(Debug)]
        struct DropCounter {
            count: Rc<RefCell<usize>>,
        }

        impl Drop for DropCounter {
            fn drop(&mut self) {
                // Increment the drop count.
                *self.count.borrow_mut() += 1;
            }
        }

        // Drop counter with 0 count initially.
        let drop_count = Rc::new(RefCell::new(0));

        let mut alloc_vec: AllocVec<DropCounter> = AllocVec::new_allocate(3);

        // Reference 3 elements to the same drop counter.
        alloc_vec.store_next(DropCounter { count: Rc::clone(&drop_count) });
        alloc_vec.store_next(DropCounter { count: Rc::clone(&drop_count) });
        alloc_vec.store_next(DropCounter { count: Rc::clone(&drop_count) });

        assert_eq!(alloc_vec.len(), 3);

        // Drop the vector
        drop(alloc_vec);

        // Since the `drop` has been called, vector should have called drop on all elements,
        // so the drop count must be 3.
        assert_eq!(*drop_count.borrow(), 3);
    }

    #[test]
    fn test_alloc_vec_memory_usage() {
        let vec: AllocVec<u8> = AllocVec::new_allocate(10);
        let expected_memory_usage = size_of::<usize>() * 3 + 10 * size_of::<i8>();
        assert_eq!(vec.memory_usage(), expected_memory_usage);
    }

    #[test]
    fn test_alloc_vec_clone_empty() {
        let original: AllocVec<u8> = AllocVec::new();
        let cloned = original.clone();

        // Cloned must have the same length and capacity
        assert_eq!(cloned.len(), 0);
        assert_eq!(cloned.capacity(), 0);

        // They must be equal (ptr is dangling in both)
        assert_eq!(cloned, original);
    }

    #[test]
    fn test_alloc_vec_clone() {
        let mut original: AllocVec<u8> = AllocVec::new_allocate(10);
        original.store_next(1);
        original.store_next(2);
        original.store_next(3);

        // Clone with the same capacity
        let mut cloned = original.clone();

        // Cloned must have the same length and capacity
        assert_eq!(cloned.len(), original.len());
        assert_eq!(cloned.capacity(), original.capacity());

        // The elements in the clone must be the same as in the original
        for i in 0..original.len() {
            assert_eq!(cloned[i], original[i]);
        }

        // Mutating the clone must not affect the original
        cloned.store_next(4);
        assert_eq!(cloned.len(), original.len() + 1);
        assert_eq!(original.len(), 3); // original length
    }

    #[test]
    fn test_alloc_vec_clone_compact() {
        let mut original: AllocVec<u8> = AllocVec::new_allocate(10);

        original.store_next(1);
        original.store_next(2);
        original.store_next(3);

        // Clone without retaining the capacity
        let cloned = original.clone_compact();

        // Cloned must have the same length as the original
        assert_eq!(cloned.len(), original.len());

        // Cloned must have a capacity equal to the length of the original
        assert_eq!(cloned.capacity(), original.len());

        // The elements in the clone must be the same as in the original
        for i in 0..original.len() {
            assert_eq!(cloned[i], original[i]);
        }

        // Mutating the clone must not affect the original
        let mut cloned = cloned; // make mutable

        // Increase the capacity of the clone by 1
        cloned.reallocate(4);

        // Compare the capacities of the clone and the original
        assert_eq!(cloned.capacity(), original.len() + 1);

        // Add a new element
        cloned.store_next(4);

        // Compare the lengths of the clone and the original
        assert_eq!(cloned.len(), original.len() + 1);
    }

    #[test]
    fn test_alloc_vec_equality() {
        let mut vec1: AllocVec<u8> = AllocVec::new_allocate(3);
        vec1.store_next(1);
        vec1.store_next(2);
        vec1.store_next(3);

        let mut vec2: AllocVec<u8> = AllocVec::new_allocate(3);
        vec2.store_next(1);
        vec2.store_next(2);
        vec2.store_next(3);

        // Vectors with the same elements must be equal
        assert_eq!(vec1, vec2);

        let mut vec3: AllocVec<u8> = AllocVec::new_allocate(3);
        vec3.store_next(4);
        vec3.store_next(5);
        vec3.store_next(6);

        // Vectors with different elements must not be equal
        assert_ne!(vec1, vec3);

        let mut vec4: AllocVec<u8> = AllocVec::new_allocate(4);
        vec4.store_next(1);
        vec4.store_next(2);
        vec4.store_next(3);

        // Vectors with the same elements but different capacities must be equal
        assert_eq!(vec1, vec4);
    }

    #[test]
    fn test_alloc_vec_debug() {
        let mut vec: AllocVec<u8> = AllocVec::new_allocate(3);
        vec.store_next(1);
        vec.store_next(2);
        vec.store_next(3);

        let debug_output = format!("{:?}", vec);
        let expected_output = "[1, 2, 3]";

        assert_eq!(debug_output, expected_output);
    }
}
