use std::alloc::{self, alloc, Layout};
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut, Range};
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
/// - The count must not be `0`.
///
#[cfg(debug_assertions)]
fn debug_assert_allocated<T>(instance: &BufferPointer<T>) {
    // Note: ptr.is_null() and ptr::null() are unstable as const functions,
    // so this fn can't be made const yet, and we can't use it in const functions.
    assert!(!instance.ptr.is_null(), "Pointer must not be null.");
    assert_ne!(instance.count, 0, "Count must not be zero.");
}

/// Debug-mode check to check the allocation state.
/// This function is only available in debug builds.
///
/// Conditions:
///
/// - The pointer must be null.
///
/// - The count must be `0`.
///
#[cfg(debug_assertions)]
fn debug_assert_not_allocated<T>(instance: &BufferPointer<T>) {
    // Note: ptr.is_null() and ptr::null() are unstable as const functions,
    // so this fn can't be made const yet, and we can't use it in const functions.
    assert!(instance.ptr.is_null(), "Pointer must be null.");
    assert_eq!(instance.count, 0, "Count must be zero.");
}

/// `BufferPointer` represents an indirect reference to _one or more_ values of type `T`
/// consecutively in memory.
///
/// `BufferPointer` guarantees proper `alignment` and `size` of `T`, and valid values of type `T`
/// when storing or loading elements.
///
/// Contrasted with other pointer types, `BufferPointer` stores the count of the elements it can
/// refer to (`count`), and the number of the initialized elements (`len`).
///
/// This buffer uses the registered `#[global_allocator]` to allocate memory.
///
/// Using custom allocators will be supported in the future, when the allocator API stabilizes.
///
/// The lifecycle of the elements is automatically managed by the `BufferPointer`.
///
/// When the `BufferPointer` is dropped, it will call `drop` on each initialized element to release
/// their resources before deallocating the memory space.
///
/// # Safety
///
/// - The total size of the allocated memory when rounded up to the nearest multiple of `align`,
///   must be less than or equal to `isize::MAX`.
///
///   If the total size exceeds `isize::MAX` bytes, the memory allocation will fail.
///
/// - Some methods can be considered `safe`, others might be considered `unsafe`.
///
///   Ultimately, the safety of the methods depends on the adherence to the API contract, and
///   satisfying the preconditions.
///
///   For now, all public methods are currently marked as safe, especially that all preconditions
///   are checked in `debug builds`.
///
/// # Internal representation:
///
/// - `ptr` is a raw pointer to the allocated memory space.
/// - `len` is the number of elements in the pointer.
/// - `count` is the number of elements the pointer can hold.
///
/// ```text
///        raw ptr  +  len   +  count     --
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
pub(crate) struct BufferPointer<T> {
    ptr: *const T,
    count: usize,
    len: usize,
    _marker: PhantomData<T>,
}

impl<T> BufferPointer<T> {

    /// Creates a new `BufferPointer` without allocating memory.
    /// The count and length are set to `0`.
    ///
    #[must_use]
    #[inline]
    pub(crate) const fn new() -> Self {
        // New instance with no allocation.
        BufferPointer {
            ptr: ptr::null(),
            count: 0,
            len: 0,
            _marker: PhantomData,
        }
    }

    /// Creates a new `BufferPointer` with the specified count.
    ///
    /// Memory is allocated for the specified count, and the length is set to `0`.
    ///
    /// # Arguments
    ///
    /// - `count` - The count of the new `BufferPointer`.
    ///
    /// # Panics
    ///
    /// - When `count` rounded up to the nearest multiple of `align` overflows `isize::MAX`.
    ///
    /// - When the allocator refuses to allocate memory space, this can happen when the system is
    ///   out of memory or the size of the requested block is too large.
    ///
    #[must_use]
    #[inline]
    pub(crate) fn new_allocate(count: usize) -> Self {
        // New instance with no allocation
        let mut instance = Self::new();

        // No allocation required
        if count == 0 {
            return instance;
        };

        // Allocate memory space
        instance.allocate(count);

        // Return the new instance
        instance
    }

    /// Creates a new `BufferPointer` with the specified count and populates it with the default
    /// value of `T`.
    ///
    /// Memory is allocated for the specified count, and the length is set to the count.
    ///
    /// # Arguments
    ///
    /// - `count` - The count of the new `BufferPointer`.
    ///
    /// # Panics
    ///
    /// - When `count` rounded up to the nearest multiple of `align` overflows `isize::MAX`.
    ///
    /// - When the allocator refuses to allocate memory space, this can happen when the system is
    ///   out of memory or the size of the requested block is too large.
    ///
    #[must_use]
    #[inline]
    pub(crate) fn new_allocate_default(count: usize) -> Self
    where T: Default
    {
        // New instance with no allocation
        let mut instance = Self::new();

        // No allocation required
        if count == 0 {
            return instance;
        }

        // Allocate memory space
        instance.allocate(count);

        // Set all elements to the default value of T
        instance.memset_default();

        // Return the new instance
        instance
    }

    /// Returns the allocated count of the `BufferPointer`.
    #[inline]
    pub(crate) const fn count(&self) -> usize {
        self.count
    }

    /// Returns the number of **initialized** elements of the `BufferPointer`.
    #[inline]
    pub(crate) const fn len(&self) -> usize {
        self.len
    }

    /// Returns `true` if the `BufferPointer` is empty.
    #[inline]
    pub(crate) const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Allocates memory space for the `BufferPointer`.
    ///
    /// # Safety
    ///
    /// - Pointer must be `null` and the current count must be `0`.
    ///   This method doesn't deallocate the old memory space pointed by the pointer.
    ///   Calling this method with a non-null pointer might cause memory leaks.
    ///   This condition is checked in debug mode only.
    ///
    /// - `count` must be greater than `0`.
    ///   This condition is checked in debug mode only.
    ///
    /// - `count`, when rounded up to the nearest multiple of `align`, must be less than or
    ///   equal to `isize::MAX`.
    ///   This condition is checked in debug mode only.
    ///
    pub(crate) fn allocate(&mut self, count: usize) {
        // Pointer must be null and the current count must be 0
        #[cfg(debug_assertions)]
        debug_assert_not_allocated(self);

        // Not allowed to allocate zero count
        debug_assert_ne!(count, 0, "Requested count must be greater than 0");

        // New layout
        let layout = unsafe {
            let layout_size = count.unchecked_mul(size_of::<T>());

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

        // Update the pointer and count
        self.ptr = ptr;
        self.count = count;
    }

    /// Shrinks or grows the allocated memory space to the specified count.
    ///
    /// # Safety
    ///
    /// - Pointer must be allocated and the current count must be greater than `0`.
    ///   This condition is checked in debug mode only.
    ///
    /// - `new_count`, when rounded up to the nearest multiple of `align`, must be less than or
    ///   equal to `isize::MAX`.
    ///   This condition is checked in debug mode only.
    ///
    /// - `new_count` must be greater than or equal to the current length.
    ///   Reallocating count less than the current length might cause memory leaks, as the
    ///   elements will be out of bounds without being dropped properly.
    ///   This condition is checked in debug mode only.
    ///
    pub(crate) fn reallocate(&mut self, new_count: usize) {
        #[cfg(debug_assertions)]
        debug_assert_allocated(self);

        // Reallocating count less than the current length is not allowed.
        debug_assert!(
            new_count >= self.len,
            "New count must be greater than or equal to the current length."
        );

        let t_size = size_of::<T>(); //  const
        let t_align = align_of::<T>(); // const

        // New size
        let new_size = unsafe {
            new_count.unchecked_mul(t_size)
        };

        // Debug-mode check for the new layout
        #[cfg(debug_assertions)]
        debug_layout_size_align(new_size, t_align);

        // Current layout
        let layout = unsafe {
            // Already checked in the `allocate_layout` function
            let current_size = self.count.unchecked_mul(t_size);
            Layout::from_size_align_unchecked(current_size, t_align)
        };

        // Reallocate memory space
        let new_ptr = unsafe {
            alloc::realloc(self.ptr as *mut u8, layout, new_size) as *mut T
        };

        // Check if reallocation failed
        if new_ptr.is_null() {
            // Reallocate failed
            alloc::handle_alloc_error(layout);
        }

        // Update the pointer and count
        self.ptr = new_ptr;
        self.count = new_count;
    }

    /// Sets all elements in the allocated memory space to the default value of `T`.
    /// The length will be updated to the current count.
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
    /// _O_(n) where n is current count of the `BufferPointer`.
    ///
    #[inline]
    pub(crate) fn memset_default(&mut self)
    where T: Default
    {
        // Write the value to all elements
        unsafe {
            for i in 0..self.count {
                ptr::write((self.ptr as *mut T).add(i), T::default());
            }
        }

        // Update length
        self.len = self.count;
    }

    /// Stores a value after the last initialized element.
    ///
    /// # Safety
    ///
    /// This method will **not** grow the count automatically.
    ///
    /// The caller must ensure that the `BufferPointer` has enough count to hold the new element.
    ///
    /// Calling this method without enough count will cause termination with `SIGSEGV`.
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
    #[inline(always)]
    pub(crate) const fn store_next(&mut self, value: T) {
        // count > len, so the pointer is not null.
        debug_assert!(self.len < self.count, "Allocated count is exhausted.");

        // Write the value to the next uninitialized element.
        unsafe {
            ptr::write((self.ptr as *mut T).add(self.len), value);
        }

        // Update length
        self.len += 1;
    }

    /// Returns a reference to an initialized element at the specified index.
    /// The index must be within the bounds of the initialized elements.
    ///
    /// # Safety
    ///
    /// - Pointer must be allocated before calling this method.
    ///   Calling this method with a null pointer will cause termination with `SIGSEGV`.
    ///
    /// - Index must be within the bounds of the initialized elements.
    ///   Loading an uninitialized elements as `T` is `undefined behavior`.
    ///
    /// These conditions are checked in debug mode only.
    ///
    /// # Time Complexity
    ///
    /// _O_(1).
    ///
    #[must_use]
    #[inline(always)]
    pub(crate) const fn load(&self, index: usize) -> &T {
        // Len > index, so the pointer is not null.
        debug_assert!(index < self.len, "Index out of bounds");

        unsafe { &*self.ptr.add(index) }
    }

    /// Returns a mutable reference to an initialized element at the specified index.
    /// The index must be within the bounds of the initialized elements.
    ///
    /// # Safety
    ///
    /// - Pointer must be allocated before calling this method.
    ///   Calling this method with a null pointer will cause termination with `SIGSEGV`.
    ///
    /// - Index must be within the bounds of the initialized elements.
    ///   Loading an uninitialized elements as `T` is `undefined behavior`.
    ///
    /// These conditions are checked in debug mode only.
    ///
    /// # Time Complexity
    ///
    /// _O_(1).
    ///
    #[must_use]
    #[inline(always)]
    pub(crate) const fn load_mut(&mut self, index: usize) -> &mut T {
        // Len > index, so the pointer is not null.
        debug_assert!(index < self.len, "Index out of bounds");

        unsafe { &mut *(self.ptr as *mut T).add(index) }
    }

    /// Returns a slice of the initialized elements within the specified range.
    /// The range must be within the bounds of the initialized elements.
    ///
    /// # Safety
    ///
    /// - Pointer must be allocated before calling this method.
    ///   Calling this method with a null pointer will cause termination with `SIGSEGV`.
    ///
    /// - Range must be within the bounds of the initialized elements.
    ///   Loading an uninitialized elements as values of `T` is `undefined behavior`.
    ///
    /// These conditions are checked in debug mode only.
    ///
    /// # Time Complexity
    ///
    /// _O_(1).
    ///
    #[must_use]
    #[inline]
    pub(crate) const fn load_range(&self, range: Range<usize>) -> &[T] {
        // Range must be valid.
        debug_assert!(
            range.start <= range.end,
            "Invalid range: start is greater than end"
        );

        // Range must be within the bounds of the initialized elements.
        debug_assert!(self.len > 0 && self.len >= range.end, "Range is out of bounds");

        unsafe {
            std::slice::from_raw_parts(self.ptr.add(range.start), range.end - range.start)
        }
    }

    /// Returns a reference to the first initialized element.
    ///
    ///
    /// # Safety
    ///
    /// This method checks for out of bounds access in debug mode only.
    ///
    /// The caller must ensure that the `BufferPointer` is not empty.
    ///
    /// # Time Complexity
    ///
    /// _O_(1).
    ///
    #[must_use]
    #[inline(always)]
    pub(crate) const fn load_first(&self) -> &T {
        // Len > 0, so the pointer is not null.
        debug_assert!(self.len > 0, "Index out of bounds");
        unsafe { &*self.ptr }
    }

    /// Returns a reference to the last initialized element.
    ///
    /// # Safety
    ///
    /// This method checks for out of bounds access in debug mode only.
    ///
    /// The caller must ensure that the `BufferPointer` is not empty.
    ///
    /// # Time Complexity
    ///
    /// _O_(1).
    ///
    #[must_use]
    #[inline(always)]
    pub(crate) const fn load_last(&self) -> &T {
        // Len > 0, so the pointer is not null.
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
    /// _O_(n) where n is the length of the `BufferPointer` minus the index.
    ///
    pub(crate) const fn take(&mut self, index: usize) -> T {
        // Len > index, so the pointer is not null.
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
    /// _O_(n) where n is the length of the `BufferPointer` minus 1.
    ///
    #[inline(always)]
    pub(crate) const fn take_first(&mut self) -> T {
        // Debug-mode checked for out-of-bounds access.
        self.take(0)
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
    #[inline(always)]
    pub(crate) const fn take_last(&mut self) -> T {
        // Len > 0, so the pointer is not null.
        debug_assert!(self.len > 0, "Index out of bounds");
        self.len -= 1;
        unsafe { ptr::read(self.ptr.add(self.len)) }
    }

    /// Calls `drop` on all initialized elements and sets the length to `0`.
    /// If there are no initialized elements, this method will do nothing.
    ///
    /// # Safety
    ///
    /// Pointer must be allocated and the current count must be greater than `0`.
    /// This condition is checked in debug mode only.
    ///
    /// # Time Complexity
    ///
    /// _O_(n) where n is the length of the `BufferPointer`.
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
    #[inline(always)]
    pub(crate) const fn replace(&mut self, index: usize, new_value: T) -> T {
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

    /// Returns an iterator over the chunks of the `BufferPointer`.
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

    /// Returns an iterator over the mutable chunks of the `BufferPointer`.
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

    /// Returns an iterator over the elements of the `BufferPointer`.
    /// If the `BufferPointer` is empty, the iterator will return an empty slice.
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

    /// Returns a mutable iterator over the initialized elements of the `BufferPointer`.
    /// If the `BufferPointer` is empty, the iterator will return an empty slice.
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

    /// Returns the current memory usage of the `BufferPointer` in bytes.
    ///
    /// The result is sum of the size of the metadata (ptr, count and len) and the size of the
    /// allocated elements.
    ///
    /// > Note:
    /// > The result is only an approximation of the memory usage.
    /// > For example, if `T` is `Box<A>`, the memory usage of `A` will not be included.
    ///
    #[must_use]
    #[inline]
    pub(crate) fn memory_usage(&self) -> usize {
        // Size of the metadata (ptr, count and len)
        let metadata_size = size_of::<usize>() * 3;
        // Size of the allocated elements
        let elements_size = self.count * size_of::<T>();
        // Total memory usage
        metadata_size + elements_size
    }
}

impl<T> Drop for BufferPointer<T> {
    /// Calls drop on each element and deallocates the memory space.
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe {
                // Current layout
                let layout = Layout::from_size_align_unchecked(
                    self.count * size_of::<T>(),
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

impl<T> Default for BufferPointer<T> {
    /// Returns the new `BufferPointer` with a count of 0.
    #[must_use]
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<'a, T> IntoIterator for &'a BufferPointer<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    /// Returns an iterator over the initialized elements of the `BufferPointer`.
    fn into_iter(self) -> Self::IntoIter {
        // This call is safe even if the pointer is null.
        self.iter()
    }
}

impl<'a, T> IntoIterator for &'a mut BufferPointer<T> {
    type Item = &'a mut T;
    type IntoIter = std::slice::IterMut<'a, T>;

    /// Returns a mutable iterator over the initialized elements of the `BufferPointer`.
    fn into_iter(self) -> Self::IntoIter {
        // This call is safe even if the pointer is null.
        self.iter_mut()
    }
}

/// An iterator over the initialized elements of the `BufferPointer`.
pub(crate) struct BufferPointerIntoIter<T> {
    buff: BufferPointer<T>,
    index: usize,
}

impl<T> Iterator for BufferPointerIntoIter<T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        // If len > 0, then the pointer is not null.
        if self.index < self.buff.len {
            unsafe {
                let item = ptr::read(self.buff.ptr.add(self.index));
                self.index += 1;
                Some(item)
            }
        } else {
            None
        }
    }
}

impl<T> IntoIterator for BufferPointer<T> {
    type Item = T;
    type IntoIter = BufferPointerIntoIter<T>;

    /// Consumes the `BufferPointer` and returns an iterator over its initialized elements.
    fn into_iter(self) -> Self::IntoIter {
        BufferPointerIntoIter {
            buff: self,
            index: 0,
        }
    }
}

impl<T> Deref for BufferPointer<T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        if self.len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
        }
    }
}

impl<T> DerefMut for BufferPointer<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        if self.len == 0 {
            &mut []
        } else {
            unsafe { std::slice::from_raw_parts_mut(self.ptr as *mut T, self.len) }
        }
    }
}

impl<T: PartialEq> PartialEq for BufferPointer<T> {
    fn eq(&self, other: &Self) -> bool {
        if self.len != other.len {
            return false;
        }
        self.iter().zip(other.iter()).all(|(a, b)| a.eq(b))
    }
}

impl<T: Clone> BufferPointer<T> {
    /// Clones the `BufferPointer` with two possible modes: `compact` or `full`.
    fn clone_in(&self, compact: bool) -> Self {
        // New instance with no allocation.
        let mut new_vec = BufferPointer {
            ptr: ptr::null(),
            count: 0,
            len: 0,
            _marker: PhantomData,
        };

        // No allocation required either way
        if self.count == 0 || (compact && self.len == 0) {
            return new_vec;
        }

        // count here must be greater than 0 either way (self.count or self.len)
        let count = if compact { self.len } else { self.count };

        // Allocate memory space
        new_vec.allocate(count);

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

    /// Clones the `BufferPointer` with count equal to the length.
    #[must_use]
    pub(crate) fn clone_compact(&self) -> Self {
        self.clone_in(true)
    }
}

impl<T: Clone> Clone for BufferPointer<T> {
    fn clone(&self) -> Self {
        self.clone_in(false)
    }
}

impl<T: Debug> Debug for BufferPointer<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buffer_ptr_new() {
        let buffer_ptr: BufferPointer<u8> = BufferPointer::new();

        assert!(buffer_ptr.ptr.is_null());
        assert_eq!(buffer_ptr.count(), 0);
        assert_eq!(buffer_ptr.len(), 0);
    }

    #[test]
    fn test_buffer_ptr_new_allocate() {
        let buffer_ptr: BufferPointer<u8> = BufferPointer::new_allocate(10);

        assert!(!buffer_ptr.ptr.is_null());
        assert_eq!(buffer_ptr.count(), 10);
        assert_eq!(buffer_ptr.len(), 0);
    }

    #[test]
    #[cfg(debug_assertions)]
    fn test_buffer_ptr_new_allocate_zero_count() {
        let buffer_ptr: BufferPointer<u8> = BufferPointer::new_allocate(0);

        // count is 0, no allocation should have been made
        assert!(buffer_ptr.ptr.is_null());
        assert_eq!(buffer_ptr.count(), 0);
        assert_eq!(buffer_ptr.len(), 0);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Size exceeds maximum limit on this platform")]
    fn test_buffer_ptr_new_allocate_overflow() {
        let _: BufferPointer<u8> = BufferPointer::new_allocate(isize::MAX as usize + 1);
    }

    #[test]
    fn test_buffer_ptr_allocate() {
        let mut buffer_ptr: BufferPointer<u8> = BufferPointer::new();

        // Allocate memory space
        buffer_ptr.allocate(10);

        assert!(!buffer_ptr.ptr.is_null());
        assert_eq!(buffer_ptr.count(), 10);
        assert_eq!(buffer_ptr.len(), 0);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Requested count must be greater than 0")]
    fn test_buffer_ptr_allocate_zero_count() {
        let mut buffer_ptr: BufferPointer<u8> = BufferPointer::new();

        // count must be greater than 0, should panic
        buffer_ptr.allocate(0);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Size exceeds maximum limit on this platform")]
    fn test_buffer_ptr_allocate_overflow() {
        let mut buffer_ptr: BufferPointer<u8> = BufferPointer::new();

        // Size exceeds maximum limit, should panic
        buffer_ptr.allocate(isize::MAX as usize + 1);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Pointer must be null.")]
    fn test_buffer_ptr_allocate_allocated() {
        let mut buffer_ptr: BufferPointer<u8> = BufferPointer::new();

        // Not yet allocated, should not panic
        buffer_ptr.allocate(1);

        assert!(!buffer_ptr.ptr.is_null());
        assert_eq!(buffer_ptr.count(), 1);

        // Already allocated, should panic
        buffer_ptr.allocate(2);
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
    fn test_buffer_ptr_memset_default() {
        let mut buffer_ptr: BufferPointer<Choice> = BufferPointer::new_allocate(10);
        assert_eq!(buffer_ptr.count(), 10);
        assert_eq!(buffer_ptr.len(), 0);

        // Set all elements to the default value of `Choice`
        buffer_ptr.memset_default();

        // Len was 0, so it should be updated to 10
        assert_eq!(buffer_ptr.len(), 10);

        // Values were uninit, so they should be set to `Default`
        for i in 0..10 {
            assert!(matches!(buffer_ptr[i], Choice::Default))
        }
    }

    #[test]
    fn test_buffer_ptr_new_allocate_default() {
        let count = 5;
        let buffer_ptr: BufferPointer<Choice> = BufferPointer::new_allocate_default(count);

        // Memory space should have been allocated
        assert!(!buffer_ptr.ptr.is_null());
        assert_eq!(buffer_ptr.count(), count);
        assert_eq!(buffer_ptr.len(), count);

        // All elements are must have been initialized to their default values
        for i in 0..count {
            assert!(matches!(buffer_ptr[i], Choice::Default))
        }
    }

    #[test]
    fn test_buffer_ptr_new_allocate_default_zero_count() {
        let buffer_ptr: BufferPointer<u8> = BufferPointer::new_allocate_default(0);

        // count is 0, no allocation should have been made
        assert!(buffer_ptr.ptr.is_null());
        assert_eq!(buffer_ptr.count(), 0);
        assert_eq!(buffer_ptr.len(), 0);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Size exceeds maximum limit on this platform")]
    fn test_buffer_ptr_new_allocate_default_overflow() {
        let _: BufferPointer<u8> = BufferPointer::new_allocate_default(isize::MAX as usize + 1);
    }

    #[test]
    fn test_buffer_ptr_reallocate() {
        let mut buffer_ptr: BufferPointer<u8> = BufferPointer::new_allocate(3);
        assert_eq!(buffer_ptr.count(), 3);

        buffer_ptr.store_next(1);
        buffer_ptr.store_next(2);
        buffer_ptr.store_next(3);

        assert_eq!(buffer_ptr.len(), 3);

        // Grows the count to 5
        buffer_ptr.reallocate(5);

        assert_eq!(buffer_ptr.count(), 5);

        // Check values after reallocation
        for i in 0..3 {
            assert_eq!(buffer_ptr[i], i as u8 + 1);
        }
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Pointer must not be null.")]
    fn test_buffer_ptr_reallocate_null_ptr() {
        let mut buffer_ptr: BufferPointer<u8> = BufferPointer::new();

        // Not yet allocated, should panic
        buffer_ptr.reallocate(10);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "New count must be greater than or equal to the current length")]
    fn test_buffer_ptr_reallocate_less_than_len() {
        let mut buffer_ptr: BufferPointer<u8> = BufferPointer::new_allocate(3);
        buffer_ptr.store_next(1);
        buffer_ptr.store_next(2);
        buffer_ptr.store_next(3);

        // New count is less than the current length, should panic
        buffer_ptr.reallocate(2);
    }

    #[test]
    fn test_buffer_ptr_store_next() {
        let mut buffer_ptr: BufferPointer<u8> = BufferPointer::new_allocate(10);
        buffer_ptr.store_next(2);
        assert_eq!(buffer_ptr.len(), 1);

        let pushed_value = unsafe { *buffer_ptr.ptr };

        assert_eq!(pushed_value, 2);
    }

    #[test]
    fn test_buffer_ptr_load() {
        let mut buffer_ptr: BufferPointer<u8> = BufferPointer::new_allocate(10);
        buffer_ptr.store_next(1);
        buffer_ptr.store_next(2);
        assert_eq!(buffer_ptr.load(0), &1);
        assert_eq!(buffer_ptr.load(1), &2);
    }


    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Index out of bounds")]
    fn test_buffer_ptr_load_out_of_bounds() {
        let buffer_ptr: BufferPointer<u8> = BufferPointer::new_allocate(10);
        let _ = buffer_ptr.load(0);
    }

    #[test]
    fn test_buffer_ptr_load_mut() {
        let mut buffer_ptr: BufferPointer<u8> = BufferPointer::new_allocate(10);
        buffer_ptr.store_next(1);
        buffer_ptr.store_next(2);
        *buffer_ptr.load_mut(0) = 10;
        assert_eq!(buffer_ptr[0], 10);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Index out of bounds")]
    fn test_buffer_ptr_load_mut_out_of_bounds() {
        let mut buffer_ptr: BufferPointer<u8> = BufferPointer::new_allocate(10);
        *buffer_ptr.load_mut(0) = 10;
    }

    #[test]
    fn test_buffer_ptr_load_range() {
        let mut buffer_ptr: BufferPointer<u8> = BufferPointer::new_allocate(10);
        buffer_ptr.store_next(1);
        buffer_ptr.store_next(2);
        buffer_ptr.store_next(3);
        buffer_ptr.store_next(4);
        let slice = buffer_ptr.load_range(1..3);
        assert_eq!(slice, &[2, 3]);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Invalid range: start is greater than end")]
    fn test_buffer_ptr_load_range_invalid_range() {
        let buffer_ptr: BufferPointer<u8> = BufferPointer::new_allocate(10);
        let _ = buffer_ptr.load_range(3..1);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Range is out of bounds")]
    fn test_buffer_ptr_load_range_out_of_bounds() {
        let buffer_ptr: BufferPointer<u8> = BufferPointer::new_allocate(10);
        let _ = buffer_ptr.load_range(0..1);
    }

    #[test]
    fn test_buffer_ptr_load_first() {
        let mut buffer_ptr: BufferPointer<u8> = BufferPointer::new_allocate(10);
        buffer_ptr.store_next(1);
        buffer_ptr.store_next(2);
        assert_eq!(buffer_ptr.load_first(), &1);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Index out of bounds")]
    fn test_buffer_ptr_load_first_out_of_bounds() {
        let buffer_ptr: BufferPointer<u8> = BufferPointer::new_allocate(10);
        let _ = buffer_ptr.load_first();
    }

    #[test]
    fn test_buffer_ptr_load_last() {
        let mut buffer_ptr: BufferPointer<u8> = BufferPointer::new_allocate(10);
        buffer_ptr.store_next(1);
        buffer_ptr.store_next(2);
        assert_eq!(buffer_ptr.load_last(), &2);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Index out of bounds")]
    fn test_buffer_ptr_load_last_out_of_bounds() {
        let buffer_ptr: BufferPointer<u8> = BufferPointer::new_allocate(10);
        let _ = buffer_ptr.load_last();
    }

    #[test]
    fn test_buffer_ptr_take_first() {
        let mut buffer_ptr: BufferPointer<u8> = BufferPointer::new_allocate(10);
        buffer_ptr.store_next(1);
        buffer_ptr.store_next(2);
        buffer_ptr.store_next(3);
        assert_eq!(buffer_ptr.take_first(), 1);
        assert_eq!(buffer_ptr.len(), 2);
        assert_eq!(buffer_ptr[0], 2);
        assert_eq!(buffer_ptr[1], 3);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Index out of bounds")]
    fn test_buffer_ptr_take_first_out_of_bounds() {
        let mut buffer_ptr: BufferPointer<u8> = BufferPointer::new_allocate(10);
        buffer_ptr.take_first();
    }

    #[test]
    fn test_buffer_ptr_take_last() {
        let mut buffer_ptr: BufferPointer<u8> = BufferPointer::new_allocate(10);
        buffer_ptr.store_next(42);
        assert_eq!(buffer_ptr.take_last(), 42);
        assert_eq!(buffer_ptr.len(), 0);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Index out of bounds")]
    fn test_buffer_ptr_take_last_out_of_bounds() {
        let mut buffer_ptr: BufferPointer<u8> = BufferPointer::new_allocate(10);
        buffer_ptr.take_last();
    }

    #[test]
    fn test_buffer_ptr_take() {
        let mut buffer_ptr: BufferPointer<u8> = BufferPointer::new_allocate(10);
        buffer_ptr.store_next(1);
        buffer_ptr.store_next(2);
        assert_eq!(buffer_ptr.take(0), 1);
        assert_eq!(buffer_ptr.len(), 1);
        assert_eq!(buffer_ptr[0], 2);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Index out of bounds")]
    fn test_buffer_ptr_take_out_of_bounds() {
        let mut buffer_ptr: BufferPointer<u8> = BufferPointer::new_allocate(10);
        assert_eq!(buffer_ptr.take(0), 1);
    }

    #[test]
    fn test_buffer_ptr_swap() {
        let mut buffer_ptr: BufferPointer<u8> = BufferPointer::new_allocate(3);
        buffer_ptr.store_next(1);
        buffer_ptr.store_next(2);
        buffer_ptr.store_next(3);
        buffer_ptr.swap(0, 2);
        assert_eq!(buffer_ptr[0], 3);
        assert_eq!(buffer_ptr[2], 1);
    }

    #[test]
    fn test_buffer_ptr_replace() {
        let mut buffer_ptr: BufferPointer<u8> = BufferPointer::new_allocate(3);
        buffer_ptr.store_next(1);
        buffer_ptr.store_next(2);
        buffer_ptr.store_next(3);
        let old_value = buffer_ptr.replace(1, 10);
        assert_eq!(buffer_ptr[1], 10);
        assert_eq!(old_value, 2);
    }

    #[test]
    fn test_buffer_ptr_iter() {
        let mut buffer_ptr: BufferPointer<u8> = BufferPointer::new_allocate(10);
        buffer_ptr.store_next(1);
        buffer_ptr.store_next(2);
        buffer_ptr.store_next(3);
        let mut iter = buffer_ptr.iter();
        assert_eq!(iter.next(), Some(&1));
        assert_eq!(iter.next(), Some(&2));
        assert_eq!(iter.next(), Some(&3));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn test_buffer_ptr_iter_mut() {
        let mut buffer_ptr: BufferPointer<u8> = BufferPointer::new_allocate(10);
        buffer_ptr.store_next(1);
        buffer_ptr.store_next(2);
        buffer_ptr.store_next(3);
        for value in buffer_ptr.iter_mut() {
            *value *= 2;
        }
        let mut iter = buffer_ptr.iter();
        assert_eq!(iter.next(), Some(&2));
        assert_eq!(iter.next(), Some(&4));
        assert_eq!(iter.next(), Some(&6));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn test_buffer_ptr_for_loop() {
        let mut buffer_ptr: BufferPointer<u8> = BufferPointer::new_allocate(3);
        buffer_ptr.store_next(1);
        buffer_ptr.store_next(2);
        buffer_ptr.store_next(3);
        let mut sum = 0;
        // Immutable borrow
        for value in &buffer_ptr {
            sum += *value;
        }
        assert_eq!(sum, 6);
    }

    #[test]
    fn test_buffer_ptr_for_loop_mut() {
        let mut buffer_ptr: BufferPointer<u8> = BufferPointer::new_allocate(3);
        buffer_ptr.store_next(1);
        buffer_ptr.store_next(2);
        buffer_ptr.store_next(3);
        // Mutable borrow
        for value in &mut buffer_ptr {
            *value *= 2;
        }
        let mut iter = buffer_ptr.iter();
        assert_eq!(iter.next(), Some(&2));
        assert_eq!(iter.next(), Some(&4));
        assert_eq!(iter.next(), Some(&6));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn test_buffer_ptr_into_iterator(){
        let mut buffer_ptr: BufferPointer<u8> = BufferPointer::new_allocate(3);
        buffer_ptr.store_next(1);
        buffer_ptr.store_next(2);
        buffer_ptr.store_next(3);
        let mut iter: BufferPointerIntoIter<u8> = buffer_ptr.into_iter();
        assert_eq!(iter.next(), Some(1));
        assert_eq!(iter.next(), Some(2));
        assert_eq!(iter.next(), Some(3));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn test_buffer_ptr_deref_empty() {
        let buffer_ptr: BufferPointer<u8> = BufferPointer::new();
        let slice: &[u8] = &*buffer_ptr;
        assert!(slice.is_empty());
    }

    #[test]
    fn test_buffer_ptr_deref() {
        let mut buffer_ptr: BufferPointer<u8> = BufferPointer::new_allocate(10);
        buffer_ptr.store_next(1);
        buffer_ptr.store_next(2);
        buffer_ptr.store_next(3);
        let slice: &[u8] = &*buffer_ptr;
        assert_eq!(slice, &[1, 2, 3]);
    }

    #[test]
    fn test_buffer_ptr_deref_mut_empty() {
        let mut buffer_ptr: BufferPointer<u8> = BufferPointer::new();
        let slice: &mut [u8] = &mut *buffer_ptr;
        assert!(slice.is_empty());
    }

    #[test]
    fn test_buffer_ptr_deref_mut() {
        let mut buffer_ptr: BufferPointer<u8> = BufferPointer::new_allocate(10);
        buffer_ptr.store_next(1);
        buffer_ptr.store_next(2);
        buffer_ptr.store_next(3);
        let slice: &mut [u8] = &mut *buffer_ptr;
        slice[0] = 10;
        assert_eq!(slice, &[10, 2, 3]);
    }

    #[test]
    fn test_buffer_ptr_drop_init() {
        let mut buffer_ptr: BufferPointer<u8> = BufferPointer::new_allocate(10);
        buffer_ptr.store_next(1);
        buffer_ptr.store_next(2);
        buffer_ptr.store_next(3);
        buffer_ptr.drop_init();
        assert_eq!(buffer_ptr.len(), 0);
    }

    #[test]
    fn test_buffer_ptr_drop() {
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

        let mut buffer_ptr: BufferPointer<DropCounter> = BufferPointer::new_allocate(3);

        // Reference 3 elements to the same drop counter.
        buffer_ptr.store_next(DropCounter { count: Rc::clone(&drop_count) });
        buffer_ptr.store_next(DropCounter { count: Rc::clone(&drop_count) });
        buffer_ptr.store_next(DropCounter { count: Rc::clone(&drop_count) });

        assert_eq!(buffer_ptr.len(), 3);

        // Drop the buffer.
        drop(buffer_ptr);

        // Since the `drop` has been called, pointer should have called drop on all elements,
        // so the drop count must be 3.
        assert_eq!(*drop_count.borrow(), 3);
    }

    #[test]
    fn test_buffer_ptr_memory_usage() {
        let vec: BufferPointer<u8> = BufferPointer::new_allocate(10);
        let expected_memory_usage = size_of::<usize>() * 3 + 10 * size_of::<i8>();
        assert_eq!(vec.memory_usage(), expected_memory_usage);
    }

    #[test]
    fn test_buffer_ptr_clone_empty() {
        let original: BufferPointer<u8> = BufferPointer::new();
        let cloned = original.clone();

        // Cloned must have the same length and count
        assert_eq!(cloned.len(), 0);
        assert_eq!(cloned.count(), 0);

        // They must be equal (ptr is dangling in both)
        assert_eq!(cloned, original);
    }

    #[test]
    fn test_buffer_ptr_clone() {
        let mut original: BufferPointer<u8> = BufferPointer::new_allocate(10);
        original.store_next(1);
        original.store_next(2);
        original.store_next(3);

        // Clone with the same count
        let mut cloned = original.clone();

        // Cloned must have the same length and count
        assert_eq!(cloned.len(), original.len());
        assert_eq!(cloned.count(), original.count());

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
    fn test_buffer_ptr_clone_compact() {
        let mut original: BufferPointer<u8> = BufferPointer::new_allocate(10);

        original.store_next(1);
        original.store_next(2);
        original.store_next(3);

        // Clone without retaining the count
        let cloned = original.clone_compact();

        // Cloned must have the same length as the original
        assert_eq!(cloned.len(), original.len());

        // Cloned must have a count equal to the length of the original
        assert_eq!(cloned.count(), original.len());

        // The elements in the clone must be the same as in the original
        for i in 0..original.len() {
            assert_eq!(cloned[i], original[i]);
        }

        // Mutating the clone must not affect the original
        let mut cloned = cloned; // make mutable

        // Increase the count of the clone by 1
        cloned.reallocate(4);

        // Count of the clone must be equal to the length of the original + 1
        assert_eq!(cloned.count(), original.len() + 1);

        // Add a new element
        cloned.store_next(4);

        // Compare the lengths of the clone and the original
        assert_eq!(cloned.len(), original.len() + 1);
    }

    #[test]
    fn test_buffer_ptr_equality() {
        let mut vec1: BufferPointer<u8> = BufferPointer::new_allocate(3);
        vec1.store_next(1);
        vec1.store_next(2);
        vec1.store_next(3);

        let mut vec2: BufferPointer<u8> = BufferPointer::new_allocate(3);
        vec2.store_next(1);
        vec2.store_next(2);
        vec2.store_next(3);

        // pointers with the same elements must be equal
        assert_eq!(vec1, vec2);

        let mut vec3: BufferPointer<u8> = BufferPointer::new_allocate(3);
        vec3.store_next(4);
        vec3.store_next(5);
        vec3.store_next(6);

        // pointers with different elements must not be equal
        assert_ne!(vec1, vec3);

        let mut vec4: BufferPointer<u8> = BufferPointer::new_allocate(4);
        vec4.store_next(1);
        vec4.store_next(2);
        vec4.store_next(3);

        // pointers with the same elements but with different counts, must be equal
        assert_eq!(vec1, vec4);
    }

    #[test]
    fn test_buffer_ptr_debug() {
        let mut vec: BufferPointer<u8> = BufferPointer::new_allocate(3);
        vec.store_next(1);
        vec.store_next(2);
        vec.store_next(3);

        let debug_output = format!("{:?}", vec);
        let expected_output = "[1, 2, 3]";

        assert_eq!(debug_output, expected_output);
    }
}
