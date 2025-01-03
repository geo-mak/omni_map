use std::alloc::{self, alloc, Layout};
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut, Index, IndexMut, Range};
use std::ptr::{self, NonNull};
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
///      NonNull<T> |  usize |  usize     |
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
    ptr: NonNull<T>,
    cap: usize,
    len: usize,
    _marker: PhantomData<T>,
}

impl<T> AllocVec<T> {

    /// Creates a new, empty `AllocVec`.
    /// No memory is allocated until elements are pushed onto the vector.
    #[must_use]
    #[inline]
    pub(crate) fn new() -> Self {
        // New dangling vector
        AllocVec {
            ptr: NonNull::dangling(),
            cap: 0,
            len: 0,
            _marker: PhantomData,
        }
    }

    /// Creates a new `AllocVec` with the specified capacity.
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
    pub(crate) fn with_capacity(cap: usize) -> Self {
        if cap == 0 {
            return Self::new();
        }

        // New allocated vector
        AllocVec {
            ptr: Self::allocate_layout(cap),
            cap,
            len: 0,
            _marker: PhantomData,
        }
    }

    /// Allocates memory space for the vector.
    /// This method is checks for valid layout size and alignment in debug builds only.
    ///
    /// # Returns
    ///
    /// - `NonNull<T>`: A non-null pointer to the allocated memory space.
    ///
    /// # Panics
    ///
    /// - When `cap` rounded up to the nearest multiple of `align` overflows `isize::MAX`.
    ///
    /// - When the allocator refuses to allocate memory space, this can happen when the system is
    ///   out of memory or the size of the requested block is too large.
    ///
    fn allocate_layout(cap: usize) -> NonNull<T> {
        // Note: Checks are bypassed at runtime because there is no meaningful strategy to handle
        // allocation errors other than panicking. It is just too much checking for nothing.

        // New layout
        let layout = unsafe {
            let layout_size = cap.unchecked_mul(size_of::<T>());

            // Debug-mode check for the layout size and alignment
            #[cfg(debug_assertions)]
            debug_layout_size_align(layout_size, align_of::<T>());

            Layout::from_size_align_unchecked(layout_size, align_of::<T>())
        };

        // Allocate memory space
        let ptr = unsafe { alloc(layout) as *mut T };

        // Return a non-null pointer
        NonNull::new(ptr).expect("Allocation refused.")
    }

    /// Allocates the vector to a new capacity.
    ///
    /// # Safety
    ///
    /// - `cap`, when rounded up to the nearest multiple of `align`, must be less than or
    ///   equal to `isize::MAX`.
    ///
    /// - This method will allocate memory for the new capacity without dropping the old elements.
    ///   Calling this method without dropping the old elements will cause memory leaks.
    ///
    /// - The length of the vector is not updated by this method. Accessing elements out of bounds
    ///   will cause undefined behavior.
    ///
    #[inline(always)]
    fn allocate(&mut self, cap: usize) {
        self.ptr = Self::allocate_layout(cap);
        self.cap = cap;
    }

    /// Reallocates the vector to a new capacity.
    ///
    /// # Safety
    ///
    /// - `cap`, when rounded up to the nearest multiple of `align`, must be less than or
    ///   equal to `isize::MAX`.
    ///
    /// - This method will reallocate memory with the valid pointer of the old layout.
    ///   Calling this method with dangling pointer will cause termination with `SIGABRT`.
    ///
    /// - If the new capacity is less than the current length, the elements at the end of the
    ///   vector will not be dropped. This will cause memory leaks.
    ///
    /// - The length of the vector is not updated by this method. Accessing elements out of bounds
    ///   will cause undefined behavior.
    ///
    fn reallocate(&mut self, cap: usize) {
        // Note: Checks are bypassed at runtime because there is no meaningful strategy to handle
        // allocation errors other than panicking. It is just too much checking for nothing.

        let t_size = size_of::<T>(); // Size of T, const
        let t_align = align_of::<T>(); // Alignment of T, const

        // Current layout
        let current_layout = unsafe {
            // Already checked in the `allocate_layout` function
            let current_size = self.cap.unchecked_mul(t_size);
            Layout::from_size_align_unchecked(current_size, t_align)
        };

        // New size
        let new_size = unsafe {
            cap.unchecked_mul(t_size)
        };

        // Debug-mode check for the new layout
        #[cfg(debug_assertions)]
        debug_layout_size_align(new_size, t_align);

        // Reallocate memory space
        let new_ptr = unsafe {
            alloc::realloc(self.ptr.as_ptr() as *mut u8, current_layout, new_size) as *mut T
        };

        // Update the pointer and capacity
        self.ptr = NonNull::new(new_ptr).expect("Reallocation refused.");
        self.cap = cap;
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
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Grows the capacity of the `AllocVec` to the specified capacity.
    /// This method is no-op if the new capacity is less than the current capacity.
    ///
    /// # Arguments
    ///
    /// - `new_cap` - The new capacity of the `AllocVec`.
    ///
    /// # Safety
    ///
    /// `new_cap`, when rounded up to the nearest multiple of `align`, must be less than or equal
    /// to `isize::MAX`.
    ///
    /// The check for overflow is done in debug mode only.
    ///
    /// # Time Complexity
    ///
    /// _O_(n) where n is the new capacity.
    ///
    #[inline]
    pub(crate) fn grow(&mut self, new_cap: usize) {
        // New allocation.
        if self.cap == 0 {
            // Overflow check is done in debug mode only.
            self.allocate(new_cap);
            return;
        }

        // Reallocation.
        if new_cap > self.cap {
            // Overflow check is done in debug mode only.
            self.reallocate(new_cap);
        }
    }

    /// Shrinks the capacity of the `AllocVec` to the specified capacity.
    ///
    /// # Arguments
    ///
    /// - `new_cap` - The new capacity of the `AllocVec`.
    ///
    /// # Time Complexity
    ///
    /// _O_(n) where n is the new capacity of the `AllocVec`.
    ///
    #[inline]
    pub(crate) fn shrink_to(&mut self, new_cap: usize) {
        if new_cap < self.cap && new_cap >= self.len {
            // This is safe because the pointer is not dangling.
            self.reallocate(new_cap);
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
    ///
    /// _O_(n) where n is the length of the `AllocVec`.
    ///
    #[inline]
    pub(crate) fn shrink_to_fit(&mut self) {
        if self.cap > self.len {
            // This is safe because the pointer is not dangling
            self.reallocate(self.len);
        }
    }

    /// Resizes the `AllocVec` to the specified length, using the provided function to generate
    /// new elements.
    ///
    /// # Note
    ///
    /// > - If the new length is equal to the current length, this method does nothing.
    ///
    /// > - If the new length is `0`, the `AllocVec` is cleared.
    ///
    /// > - If the new length is _greater_ than the current length, the `AllocVec` is extended by
    /// >   the difference, and the new elements are generated by the provided function.
    ///
    /// > - If the new length is _less_ than the current length, the elements at the end of the
    /// >   `AllocVec` are dropped, and remaining elements will keep their values.
    /// >   However, this will *not* cause vector to shrink capacity. Allocated capacity will
    /// >   remain the same.
    ///
    /// # Arguments
    ///
    /// - `new_len` - The new length of the `AllocVec`.
    ///
    /// - `f` - The function to generate new elements.
    ///
    /// # Time Complexity
    ///
    /// _O_(n) on average where n is the new length of the `AllocVec`.
    ///
    pub(crate) fn resize_with<F>(&mut self, new_len: usize, mut f: F)
    where
        F: FnMut() -> T,
    {
        if new_len == self.len {
            // Length is equal to the current length, do nothing
            return;
        } else if new_len > self.len {
            // increase the capacity to accommodate the new length
            self.grow(new_len);

            // Fill in the new elements
            for i in self.len..new_len {
                unsafe {
                    ptr::write(self.ptr.as_ptr().add(i), f());
                }
            }

            // Update length
            self.len = new_len;

        } else {
            // Length is less than the current length
            self.truncate(new_len)
        };
    }

    /// Shortens the vector, keeping the first len elements and dropping the rest.
    /// This method has no effect, if the vector is empty or `len` is greater or equal to the
    /// vector's current length.
    ///
    /// Truncating when `len` == `0` is equivalent to calling the clear method.
    ///
    /// # Arguments
    ///
    /// - `len`: the number of elements to retain.
    ///
    /// # Time Complexity
    ///
    /// _O_(n) where n is the number of elements to drop (self.len - len).
    ///
    pub(crate) fn truncate(&mut self, len: usize) {
        if len > self.len{
            return;
        }
        unsafe {
            let drop_elements = self.len - len;
            // Get a slice of the elements to drop
            let drop_slice = ptr::slice_from_raw_parts_mut(
                self.as_mut_ptr().add(len), drop_elements
            );
            // Update length
            self.len = len;
            // Call drop on each element to release resources.
            ptr::drop_in_place(drop_slice);
        }
    }

    /// Appends an element to the back of the `AllocVec`.
    ///
    /// # Safety
    ///
    /// This method will **not** grow the capacity automatically.
    ///
    /// The caller must ensure that the `AllocVec` has enough capacity to hold the new element.
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
    pub(crate) fn push_no_grow(&mut self, value: T) {
        debug_assert!(self.cap != 0, "Capacity must be greater than 0");
        debug_assert!(self.len < self.cap, "Capacity overflow");
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
    /// - `value` - The value to append.
    ///
    /// # Returns
    ///
    /// - `Ok(())` if the element was successfully appended.
    ///
    /// - `Err(value)` if the `AllocVec` is at full capacity.
    ///
    /// # Time Complexity
    ///
    /// _O_(1).
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
    /// - `index` - The index of the element to retrieve.
    ///
    /// # Time Complexity
    ///
    /// _O_(1).
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
    /// - `index` - The index of the element to retrieve.
    ///
    /// # Time Complexity
    ///
    /// _O_(1).
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
    ///
    /// _O_(1).
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
    ///
    /// _O_(1).
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
    /// - `index` - The index of the element to remove.
    ///
    /// # Panics
    ///
    /// Panics if the index is out of bounds.
    ///
    /// # Time Complexity
    ///
    /// _O_(n) where n is the length of the `AllocVec` minus the index.
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
    ///
    /// _O_(1).
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
    ///
    /// _O_(n) where n is the length of the `AllocVec` minus 1.
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
    /// Panics if the index is out of bounds.
    ///
    /// # Time Complexity
    ///
    /// _O_(1).
    ///
    #[inline]
    pub(crate) fn replace(&mut self, index: usize, new_value: T) -> T {
        assert!(index < self.len, "Index out of bounds");
        unsafe {
            let ptr = self.ptr.as_ptr().add(index);
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
    /// Panics if either index is out of bounds.
    ///
    /// # Time Complexity
    ///
    /// _O_(1).
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
        assert!(chunk_size > 0, "Chunk size must be greater than 0");
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.len).chunks(chunk_size) }
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
        assert!(chunk_size > 0, "Chunk size must be greater than 0");
        unsafe {
            std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len).chunks_mut(chunk_size)
        }
    }

    /// Returns an iterator over the elements of the `AllocVec`.
    ///
    /// # Time Complexity
    ///
    /// _O_(1).
    ///
    #[inline]
    pub(crate) fn iter(&self) -> std::slice::Iter<'_, T> {
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.len).iter() }
    }

    /// Returns a mutable iterator over the elements of the `AllocVec`.
    ///
    /// # Time Complexity
    ///
    /// _O_(1).
    ///
    #[inline]
    pub(crate) fn iter_mut(&mut self) -> std::slice::IterMut<'_, T> {
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len).iter_mut() }
    }

    /// Clears the `AllocVec` and calls `drop` on elements.
    ///
    /// # Time Complexity
    ///
    /// _O_(n) where n is the length of the `AllocVec`.
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
    /// Drops the `AllocVec`, deallocating its memory.
    fn drop(&mut self) {
        if self.cap != 0 {
            let layout = Layout::array::<T>(self.cap).expect("Deallocation error: layout error");
            unsafe {
                // Call drop on each element to release their resources.
                ptr::drop_in_place(std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len));
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
    /// - `index` - The index of the element to retrieve.
    ///
    /// # Panics
    ///
    /// Panics if the index is out of bounds.
    ///
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
    /// - `index` - The index of the element to retrieve.
    ///
    /// # Panics
    ///
    /// Panics if the index is out of bounds.
    ///
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
        assert!(range.end <= self.len, "Range is out of bounds");
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
    pub(crate) fn with_capacity_and_populate(cap: usize) -> Self {

        // No allocation required
        if cap == 0 {
            return Self::new();
        }

        // Allocate layout
        let ptr = Self::allocate_layout(cap);

        // Initialize elements with default values
        unsafe {
            for i in 0..cap {
                ptr::write(ptr.as_ptr().add(i), T::default());
            }
        }

        // New allocated vector
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
pub(crate) struct AllocVecIntoIter<T> {
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
            alloc_vec.push_no_grow(item);
        }
        alloc_vec
    }
}

impl<T> Deref for AllocVec<T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        if self.len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
        }
    }
}

impl<T> DerefMut for AllocVec<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        if self.len == 0 {
            &mut []
        } else {
            unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
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
            ptr: NonNull::dangling(),
            cap: 0,
            len: 0,
            _marker: PhantomData,
        };

        if self.cap == 0 || (compact && self.len == 0) {
            // No allocation required either way
            return new_vec;
        }

        // cap here must be greater than 0 either way (self.cap or self.len)
        let cap = if compact { self.len } else { self.cap };

        // Set the new pointer and capacity
        new_vec.ptr = Self::allocate_layout(cap);
        new_vec.cap = cap;

        // Clone elements
        unsafe {
            let src_slice = std::slice::from_raw_parts(self.ptr.as_ptr(), self.len);
            let dest_slice = std::slice::from_raw_parts_mut(new_vec.ptr.as_ptr(), self.len);
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
        assert_eq!(alloc_vec.ptr.as_ptr(), 0x1 as *mut u8);
        assert_eq!(alloc_vec.capacity(), 0);
        assert_eq!(alloc_vec.len(), 0);
    }

    #[test]
    fn test_alloc_vec_with_capacity() {
        let alloc_vec: AllocVec<u8> = AllocVec::with_capacity(10);
        assert_eq!(alloc_vec.capacity(), 10);
        assert_eq!(alloc_vec.len(), 0);
    }

    #[test]
    #[should_panic(expected = "Size exceeds maximum limit on this platform")]
    fn test_alloc_vec_with_capacity_overflow() {
        let _: AllocVec<u8> = AllocVec::with_capacity(isize::MAX as usize + 1);
    }

    #[test]
    fn test_with_capacity_and_populate() {
        let capacity = 5;
        let alloc_vec: AllocVec<u8> = AllocVec::with_capacity_and_populate(capacity);

        // Map's length and capacity must be equal to the specified capacity
        assert_eq!(alloc_vec.len(), capacity);
        assert_eq!(alloc_vec.capacity(), capacity);

        // All elements are must have been initialized to their default values
        for i in 0..capacity {
            assert_eq!(alloc_vec[i], u8::default());
        }
    }

    #[test]
    fn test_alloc_vec_grow() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::with_capacity(10);
        assert_eq!(alloc_vec.capacity(), 10);
        alloc_vec.grow(15);
        assert_eq!(alloc_vec.capacity(), 15);
    }

    #[test]
    #[should_panic(expected = "Size exceeds maximum limit on this platform")]
    fn test_alloc_vec_grow_capacity_overflow() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::with_capacity(10);
        assert_eq!(alloc_vec.capacity(), 10);

        // Should panic as the new capacity will overflow
        alloc_vec.grow(isize::MAX as usize + 1);
    }

    #[test]
    fn test_alloc_vec_shrink() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::with_capacity(10);
        alloc_vec.push_no_grow(1);
        alloc_vec.push_no_grow(2);
        alloc_vec.push_no_grow(3);
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
        let mut alloc_vec: AllocVec<u8> = AllocVec::with_capacity(10);
        alloc_vec.push_no_grow(1);
        assert_eq!(alloc_vec.len(), 1);
    }

    #[test]
    fn test_try_push() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::with_capacity(2);
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
        let mut alloc_vec: AllocVec<u8> = AllocVec::with_capacity(10);
        alloc_vec.push_no_grow(1);
        alloc_vec.push_no_grow(2);
        alloc_vec.push_no_grow(3);
        assert_eq!(alloc_vec.get(0), Some(&1));
        assert_eq!(alloc_vec.get(1), Some(&2));
        assert_eq!(alloc_vec.get(2), Some(&3));
        assert_eq!(alloc_vec.get(3), None);
    }

    #[test]
    fn test_alloc_vec_get_mut() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::with_capacity(10);
        alloc_vec.push_no_grow(1);
        alloc_vec.push_no_grow(2);
        alloc_vec.push_no_grow(3);
        if let Some(value) = alloc_vec.get_mut(1) {
            *value = 10;
        }
        assert_eq!(alloc_vec.get(1), Some(&10));
    }

    #[test]
    fn test_alloc_vec_index() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::with_capacity(10);
        alloc_vec.push_no_grow(1);
        alloc_vec.push_no_grow(2);
        assert_eq!(alloc_vec[0], 1);
        assert_eq!(alloc_vec[1], 2);
    }

    #[test]
    #[should_panic(expected = "Index out of bounds")]
    fn test_alloc_vec_index_out_of_bounds() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::with_capacity(10);
        alloc_vec.push_no_grow(10);

        let _ = alloc_vec[1];
    }

    #[test]
    fn test_alloc_vec_index_mut() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::with_capacity(10);
        alloc_vec.push_no_grow(1);
        alloc_vec[0] = 2;
        assert_eq!(alloc_vec[0], 2);
    }

    #[test]
    fn test_alloc_vec_index_range() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::with_capacity(10);
        alloc_vec.push_no_grow(1);
        alloc_vec.push_no_grow(2);
        alloc_vec.push_no_grow(3);
        alloc_vec.push_no_grow(4);

        // Read values in the range [1, 3)
        let slice = &alloc_vec[1..3];

        // Verify the values
        assert_eq!(slice, &[2, 3]);
    }

    #[test]
    fn test_alloc_vec_index_range_mut() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::with_capacity(10);
        alloc_vec.push_no_grow(1);
        alloc_vec.push_no_grow(2);
        alloc_vec.push_no_grow(3);
        alloc_vec.push_no_grow(4);

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
        let mut alloc_vec: AllocVec<u8> = AllocVec::with_capacity(10);
        alloc_vec.push_no_grow(1);
        alloc_vec.push_no_grow(2);
        assert_eq!(alloc_vec.first(), &1);
    }

    #[test]
    #[should_panic(expected = "Index out of bounds")]
    fn test_alloc_vec_first_out_of_bounds() {
        let alloc_vec: AllocVec<u8> = AllocVec::with_capacity(10);
        let _ = alloc_vec.first();
    }

    #[test]
    fn test_alloc_vec_last() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::with_capacity(10);
        alloc_vec.push_no_grow(1);
        alloc_vec.push_no_grow(2);
        assert_eq!(alloc_vec.last(), &2);
    }

    #[test]
    #[should_panic(expected = "Index out of bounds")]
    fn test_alloc_vec_last_out_of_bounds() {
        let alloc_vec: AllocVec<u8> = AllocVec::with_capacity(10);
        let _ = alloc_vec.last();
    }

    #[test]
    fn test_alloc_vec_pop_front() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::with_capacity(10);
        alloc_vec.push_no_grow(1);
        alloc_vec.push_no_grow(2);
        alloc_vec.push_no_grow(3);
        assert_eq!(alloc_vec.pop_front(), 1);
        assert_eq!(alloc_vec.len(), 2);
        assert_eq!(alloc_vec[0], 2);
        assert_eq!(alloc_vec[1], 3);
    }

    #[test]
    #[should_panic(expected = "Index out of bounds")]
    fn test_alloc_vec_pop_front_out_of_bounds() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::with_capacity(10);
        alloc_vec.pop_front();
    }

    #[test]
    fn test_alloc_vec_pop() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::with_capacity(10);
        alloc_vec.push_no_grow(42);
        assert_eq!(alloc_vec.pop(), 42);
        assert_eq!(alloc_vec.len(), 0);
    }

    #[test]
    #[should_panic(expected = "Index out of bounds")]
    fn test_alloc_vec_pop_out_of_bounds() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::with_capacity(10);
        alloc_vec.pop();
    }

    #[test]
    fn test_alloc_vec_remove() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::with_capacity(10);
        alloc_vec.push_no_grow(1);
        alloc_vec.push_no_grow(2);
        assert_eq!(alloc_vec.remove(0), 1);
        assert_eq!(alloc_vec.len(), 1);
        assert_eq!(alloc_vec[0], 2);
    }

    #[test]
    #[should_panic(expected = "Index out of bounds")]
    fn test_alloc_vec_remove_out_of_bounds() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::with_capacity(10);
        assert_eq!(alloc_vec.remove(0), 1);
    }

    #[test]
    fn test_alloc_vec_resize_with() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::with_capacity(10);

        // Resize to 3 elements with a default value of 1
        alloc_vec.resize_with(2, || 1);

        // Vector was created empty, so the first 2 elements should be 1
        assert_eq!(alloc_vec.len(), 2);
        assert_eq!(alloc_vec[0], 1);
        assert_eq!(alloc_vec[1], 1);

        // Resize with length equals the current length (no effect)
        alloc_vec.resize_with(2, || 10);

        // The vector should remain the same
        assert_eq!(alloc_vec.len(), 2);
        assert_eq!(alloc_vec[0], 1);
        assert_eq!(alloc_vec[1], 1);

        // Resize with length greater than the current length
        alloc_vec.resize_with(3, || 10);

        // An element should have been added with the default value
        assert_eq!(alloc_vec.len(), 3);
        assert_eq!(alloc_vec[0], 1);
        assert_eq!(alloc_vec[1], 1);
        assert_eq!(alloc_vec[2], 10); // <-- New element with default value

        // Resize with length less than the current length but greater than 0
        alloc_vec.resize_with(2, || 10);

        // The last element should have been dropped
        assert_eq!(alloc_vec.len(), 2);

        // The first 2 elements should remain the same, because no rewrite was done
        assert_eq!(alloc_vec[0], 1);
        assert_eq!(alloc_vec[1], 1);

        // Resize with length equals 0
        alloc_vec.resize_with(0, || 10);

        // The vector should be empty
        assert_eq!(alloc_vec.len(), 0);
    }

    #[test]
    fn test_alloc_vec_truncate() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::new();

        assert_eq!(alloc_vec.ptr.as_ptr(), 0x1 as *mut u8);

        // Truncate prior to allocation (no effect)
        alloc_vec.truncate(0);

        // Allocate memory space for 3 elements
        alloc_vec.grow(3);

        assert_eq!(alloc_vec.len(), 0);

        // Truncate with zero length when vector is empty (no effect)
        alloc_vec.truncate(0);

        // Truncate with non-zero length when vector is empty (no effect)
        alloc_vec.truncate(3);

        // Add 2 elements
        alloc_vec.push_no_grow(1);
        alloc_vec.push_no_grow(2);

        // Truncate to length greater than current length (no effect)
        alloc_vec.truncate(3);
        assert_eq!(alloc_vec.len(), 2);
        assert_eq!(alloc_vec[0], 1);
        assert_eq!(alloc_vec[1], 2);

        // Truncate to length equal to current length (no effect)
        alloc_vec.truncate(2);
        assert_eq!(alloc_vec.len(), 2);
        assert_eq!(alloc_vec[0], 1);
        assert_eq!(alloc_vec[1], 2);

        // Truncate to length less than current length
        alloc_vec.truncate(1);
        assert_eq!(alloc_vec.len(), 1);
        assert_eq!(alloc_vec[0], 1);

        // Truncate to length 0, effectively clearing the vector
        alloc_vec.truncate(0);
        assert_eq!(alloc_vec.len(), 0);
    }

    #[test]
    fn test_alloc_vec_swap() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::with_capacity(3);
        alloc_vec.push_no_grow(1);
        alloc_vec.push_no_grow(2);
        alloc_vec.push_no_grow(3);
        alloc_vec.swap(0, 2);
        assert_eq!(alloc_vec[0], 3);
        assert_eq!(alloc_vec[2], 1);
    }

    #[test]
    fn test_alloc_vec_replace() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::with_capacity(3);
        alloc_vec.push_no_grow(1);
        alloc_vec.push_no_grow(2);
        alloc_vec.push_no_grow(3);
        let old_value = alloc_vec.replace(1, 10);
        assert_eq!(alloc_vec[1], 10);
        assert_eq!(old_value, 2);
    }

    #[test]
    fn test_alloc_vec_iter() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::with_capacity(10);
        alloc_vec.push_no_grow(1);
        alloc_vec.push_no_grow(2);
        alloc_vec.push_no_grow(3);
        let mut iter = alloc_vec.iter();
        assert_eq!(iter.next(), Some(&1));
        assert_eq!(iter.next(), Some(&2));
        assert_eq!(iter.next(), Some(&3));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn test_alloc_vec_iter_mut() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::with_capacity(10);
        alloc_vec.push_no_grow(1);
        alloc_vec.push_no_grow(2);
        alloc_vec.push_no_grow(3);
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
        let mut alloc_vec: AllocVec<u8> = AllocVec::with_capacity(10);
        alloc_vec.push_no_grow(1);
        alloc_vec.push_no_grow(2);
        alloc_vec.push_no_grow(3);
        alloc_vec.clear();
        assert_eq!(alloc_vec.len(), 0);
    }

    #[test]
    fn test_alloc_vec_for_loop() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::with_capacity(3);
        alloc_vec.push_no_grow(1);
        alloc_vec.push_no_grow(2);
        alloc_vec.push_no_grow(3);
        let mut sum = 0;
        // Immutable borrow
        for value in &alloc_vec {
            sum += *value;
        }
        assert_eq!(sum, 6);
    }

    #[test]
    fn test_alloc_vec_for_loop_mut() {
        let mut alloc_vec: AllocVec<u8> = AllocVec::with_capacity(3);
        alloc_vec.push_no_grow(1);
        alloc_vec.push_no_grow(2);
        alloc_vec.push_no_grow(3);
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
        let mut alloc_vec: AllocVec<u8> = AllocVec::with_capacity(3);
        alloc_vec.push_no_grow(1);
        alloc_vec.push_no_grow(2);
        alloc_vec.push_no_grow(3);
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
        let mut alloc_vec: AllocVec<u8> = AllocVec::with_capacity(10);
        alloc_vec.push_no_grow(1);
        alloc_vec.push_no_grow(2);
        alloc_vec.push_no_grow(3);
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
        let mut alloc_vec: AllocVec<u8> = AllocVec::with_capacity(10);
        alloc_vec.push_no_grow(1);
        alloc_vec.push_no_grow(2);
        alloc_vec.push_no_grow(3);
        let slice: &mut [u8] = &mut *alloc_vec;
        slice[0] = 10;
        assert_eq!(slice, &[10, 2, 3]);
    }

    #[test]
    fn test_alloc_vec_memory_usage() {
        let vec: AllocVec<u8> = AllocVec::with_capacity(10);
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
        let mut original: AllocVec<u8> = AllocVec::with_capacity(10);
        original.push_no_grow(1);
        original.push_no_grow(2);
        original.push_no_grow(3);

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
        cloned.push_no_grow(4);
        assert_eq!(cloned.len(), original.len() + 1);
        assert_eq!(original.len(), 3); // original length
    }

    #[test]
    fn test_alloc_vec_clone_compact() {
        let mut original: AllocVec<u8> = AllocVec::with_capacity(10);

        original.push_no_grow(1);
        original.push_no_grow(2);
        original.push_no_grow(3);

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
        cloned.grow(4);

        // Compare the capacities of the clone and the original
        assert_eq!(cloned.capacity(), original.len() + 1);

        // Add a new element
        cloned.push_no_grow(4);

        // Compare the lengths of the clone and the original
        assert_eq!(cloned.len(), original.len() + 1);
    }

    #[test]
    fn test_alloc_vec_equality() {
        let mut vec1: AllocVec<u8> = AllocVec::with_capacity(3);
        vec1.push_no_grow(1);
        vec1.push_no_grow(2);
        vec1.push_no_grow(3);

        let mut vec2: AllocVec<u8> = AllocVec::with_capacity(3);
        vec2.push_no_grow(1);
        vec2.push_no_grow(2);
        vec2.push_no_grow(3);

        // Vectors with the same elements must be equal
        assert_eq!(vec1, vec2);

        let mut vec3: AllocVec<u8> = AllocVec::with_capacity(3);
        vec3.push_no_grow(4);
        vec3.push_no_grow(5);
        vec3.push_no_grow(6);

        // Vectors with different elements must not be equal
        assert_ne!(vec1, vec3);

        let mut vec4: AllocVec<u8> = AllocVec::with_capacity(4);
        vec4.push_no_grow(1);
        vec4.push_no_grow(2);
        vec4.push_no_grow(3);

        // Vectors with the same elements but different capacities must be equal
        assert_eq!(vec1, vec4);
    }

    #[test]
    fn test_alloc_vec_debug() {
        let mut vec: AllocVec<u8> = AllocVec::with_capacity(3);
        vec.push_no_grow(1);
        vec.push_no_grow(2);
        vec.push_no_grow(3);

        let debug_output = format!("{:?}", vec);
        let expected_output = "[1, 2, 3]";

        assert_eq!(debug_output, expected_output);
    }
}
