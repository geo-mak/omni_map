use core::alloc::Layout;
use core::marker::PhantomData;
use core::ops::Range;
use core::ptr;

use std::alloc::{self, alloc};

use crate::opt::branch_prediction;

/// Debug-mode check for the layout size and alignment.
/// This function is only available in debug builds.
///
/// Conditions:
///
/// - `align` of `T` must not be zero.
///
/// - `align` of `T` must be a power of two.
///
/// - `size` must be greater than `0`.
///
/// - `size`, when rounded up to the nearest multiple of `align`, must be less than or
///   equal to `isize::MAX`.
///
#[cfg(debug_assertions)]
const fn debug_layout_size_align(size: usize, align: usize) {
    // Alignment check
    assert!(align.is_power_of_two(), "Alignment must be a power of two");

    // Size check
    assert!(size > 0, "Allocation size must be greater than 0");

    let max_size = (isize::MAX as usize + 1) - align;
    assert!(
        max_size > size,
        "Allocation size exceeds maximum limit on this platform"
    );
}

/// Debug-mode check to check the allocation state.
/// This function is only available in debug builds.
///
/// Conditions:
///
/// - The pointer must not be null.
///
#[cfg(debug_assertions)]
const fn debug_assert_allocated<T>(instance: &UnsafeBufferPointer<T>) {
    assert!(!instance.ptr.is_null(), "Pointer must not be null");
}

/// Debug-mode check to check the allocation state.
/// This function is only available in debug builds.
///
/// Conditions:
///
/// - The pointer must be null.
///
#[cfg(debug_assertions)]
const fn debug_assert_not_allocated<T>(instance: &UnsafeBufferPointer<T>) {
    assert!(instance.ptr.is_null(), "Pointer must be null");
}

/// Debug-mode check for the count.
/// This function is only available in debug builds.
///
/// Conditions:
///
/// - `copy_count` must be less than or equal to `allocated_count`.
///
#[cfg(debug_assertions)]
const fn debug_assert_copy_inbounds(allocated_count: usize, copy_count: usize) {
    assert!(
        copy_count <= allocated_count,
        "Copy count must be less than or equal to allocated count"
    );
}

/// `UnsafeBufferPointer` represents an indirect reference to _one or more_ values of type `T`
/// consecutively in memory.
///
/// `UnsafeBufferPointer` is a typed pointer, not a raw pointer, and it guarantees proper `size`
/// and `alignment` of `T`, when storing or loading values, but it doesn't guarantee safe operations
/// with measures such as null pointer checks, bounds checking, or automatic drop of initialized
/// values.
///
/// Limited checks for invariants are done in debug mode only.
///
/// `UnsafeBufferPointer` doesn't store any metadata about the allocated memory space, such as the
/// size of the allocated memory space and the number of initialized elements, therefore it doesn't
/// offer automatic memory management.
///
/// The user is responsible for allocating, reallocating, and deallocating memory.
///
/// If `T` is not of trivial type, the user is responsible for calling `drop` on the elements to
/// release resources, before deallocating the memory space.
///
/// This pointer uses the registered `#[global_allocator]` to allocate memory.
///
/// Using custom allocators will be supported in the future, when the `Allocator` API stabilizes.
///
pub(crate) struct UnsafeBufferPointer<T> {
    ptr: *const T,
    _marker: PhantomData<T>,
}

impl<T> UnsafeBufferPointer<T> {
    pub(crate) const T_SIZE: usize = size_of::<T>();
    pub(crate) const T_ALIGN: usize = align_of::<T>();

    /// Creates a new `UnsafeBufferPointer` without allocating memory.
    ///
    /// The pointer is set to `null`.
    ///
    #[must_use]
    #[inline]
    pub(crate) const fn new() -> Self {
        // New instance with no allocation.
        UnsafeBufferPointer {
            ptr: ptr::null(),
            _marker: PhantomData,
        }
    }

    /// Creates a new `UnsafeBufferPointer` with the specified `count`.
    ///
    /// Memory is allocated for the specified `count` of type `T`.
    ///
    /// # Safety
    ///
    /// - `count` must be greater than `0`.
    ///
    /// - The total size of the allocated memory when rounded up to the nearest multiple of `align`,
    ///   must be less than or equal to `isize::MAX`.
    ///
    ///   If the total size exceeds `isize::MAX` bytes, the memory allocation will fail.
    ///
    #[must_use]
    #[inline]
    pub(crate) unsafe fn new_allocate(count: usize) -> Self {
        // New instance with no allocation.
        let mut instance = Self::new();

        // Allocate memory space.
        instance.allocate(count);

        // Return the new instance.
        instance
    }

    /// Creates a new `UnsafeBufferPointer` with the specified `count` of type `T` and populates
    /// it with the default value of `T`.
    ///
    /// # Safety
    ///
    /// - `count` must be greater than `0`.
    ///
    /// - The total size of the allocated memory when rounded up to the nearest multiple of `align`,
    ///   must be less than or equal to `isize::MAX`.
    ///
    ///   If the total size exceeds `isize::MAX` bytes, the memory allocation will fail.
    ///
    #[must_use]
    #[inline]
    pub(crate) unsafe fn new_allocate_default(count: usize) -> Self
    where
        T: Default,
    {
        // New instance with no allocation.
        let mut instance = Self::new();

        // Allocate memory space.
        instance.allocate(count);

        // Set all elements to the default value of T.
        instance.memset_default(count);

        // Return the new instance.
        instance
    }

    /// Checks if the `UnsafeBufferPointer` is null.
    #[must_use]
    #[inline(always)]
    pub(crate) const fn is_null(&self) -> bool {
        self.ptr.is_null()
    }

    /// Sets the pointer to `null` and returns the current pointer.
    ///
    /// # Safety
    ///
    /// This method doesn't provide any guarantees about the state of the returned pointer and its
    /// allocated memory space.
    ///
    #[must_use]
    #[inline]
    pub(crate) const unsafe fn invalidate(&mut self) -> UnsafeBufferPointer<T> {
        let instance = UnsafeBufferPointer {
            ptr: self.ptr,
            _marker: PhantomData,
        };

        // Set the pointer to null.
        self.ptr = ptr::null();

        // Return the current.
        instance
    }

    /// Creates a new layout for the specified `count` of type `T`.
    ///
    /// This method checks for valid size and alignment in debug mode only.
    ///
    #[must_use]
    #[inline(always)]
    const unsafe fn make_layout(&self, count: usize) -> Layout {
        let size = count.unchecked_mul(Self::T_SIZE);

        #[cfg(debug_assertions)]
        debug_layout_size_align(size, Self::T_ALIGN);

        Layout::from_size_align_unchecked(size, Self::T_ALIGN)
    }

    /// Allocates memory space for the specified `count` of type `T`.
    ///
    /// # Safety
    ///
    /// - Pointer must be `null` before calling this method.
    ///   This method doesn't deallocate the allocated memory space pointed by the pointer.
    ///   Calling this method with a non-null pointer might cause memory leaks, as the allocated
    ///   memory space will be lost.
    ///
    /// - `count` must be greater than `0`.
    ///
    /// - `count` in bytes, when rounded up to the nearest multiple of `align`, must be less than
    ///   or equal to `isize::MAX` bytes.
    ///
    pub(crate) unsafe fn allocate(&mut self, count: usize) {
        #[cfg(debug_assertions)]
        debug_assert_not_allocated(self);

        let new_layout = self.make_layout(count);

        // Allocate memory space
        let ptr = alloc(new_layout) as *mut T;

        // Failure branch.
        if branch_prediction::unlikely(ptr.is_null()) {
            alloc::handle_alloc_error(new_layout);
        }

        // Update the pointer and count
        self.ptr = ptr;
    }

    /// Deallocates the memory space pointed by the pointer.
    ///
    /// This method doesn't call `drop` on the initialized elements.
    ///
    /// The pointer is set to `null` after deallocation.
    ///
    /// # Safety
    ///
    /// - Pointer must be allocated before calling this method.
    ///   Calling this method with a null pointer will cause termination with `SIGABRT`.
    ///
    /// - Initialized elements will not be dropped before deallocating memory.
    ///   This might cause memory leaks if `T` is not of trivial type, or if the elements are not
    ///   dropped properly before calling this method.
    ///
    /// - `allocated_count` must be the same as the actual allocated count of type `T`, which
    ///   implies it can't be `0` also.
    ///   If the count is not the same, the result is `undefined behavior`.
    ///
    pub(crate) unsafe fn deallocate(&mut self, allocated_count: usize) {
        #[cfg(debug_assertions)]
        debug_assert_allocated(self);

        // Current layout
        let layout = self.make_layout(allocated_count);

        // This API is weird, it takes everything and tells nothing!
        alloc::dealloc(self.ptr as *mut u8, layout);

        // Update the pointer and count
        self.ptr = ptr::null();
    }

    /// Shrinks or grows the allocated memory space to the specified `count`, and copies the
    /// initialized elements to the new memory space.
    ///
    /// # Safety
    ///
    /// - Pointer must be allocated before calling this method.
    ///   Calling this method with a null pointer will cause termination with `SIGABRT`.
    ///
    /// - `allocated_count` must be the same as the previously allocated `count` of type `T`.
    ///   If the count is not the same, the result is `undefined behavior`.
    ///
    /// - Initialized elements will not be dropped when shrinking the memory space.
    ///   This might cause memory leaks if `T` is not of trivial type, or if the elements are not
    ///   dropped properly before calling this method.
    ///
    /// - `new_count` must be greater than `0`.
    ///   Allocating memory space with `0` count will be `undefined behavior`.
    ///
    /// - `new_count` in bytes, when rounded up to the nearest multiple of `align`, must be less
    ///   than or equal to `isize::MAX` bytes.
    ///
    /// - `copy_count` must be within the bounds of the allocated memory space.
    ///   Copying more elements than the allocated count will cause termination with `SIGSEGV`.
    ///
    /// These invariants are checked in debug mode only.
    ///
    pub(crate) unsafe fn reallocate(
        &mut self,
        allocated_count: usize,
        new_count: usize,
        copy_count: usize,
    ) {
        #[cfg(debug_assertions)]
        debug_assert_allocated(self);

        #[cfg(debug_assertions)]
        debug_assert_copy_inbounds(allocated_count, copy_count);

        // Debug-mode checked for valid size and alignment.
        let new_layout = self.make_layout(new_count);

        // Allocate new memory space.
        let new_ptr = alloc(new_layout) as *mut T;

        // Success branch.
        if branch_prediction::likely(!new_ptr.is_null()) {
            ptr::copy_nonoverlapping(self.ptr, new_ptr, copy_count);

            let current_layout = self.make_layout(allocated_count);

            // Deallocate the current memory space.
            alloc::dealloc(self.ptr as *mut u8, current_layout);

            // Update the pointer.
            self.ptr = new_ptr;

            // Done.
            return;
        };

        // Allocation failed.
        // This call is just a fancy way to abort the program.
        alloc::handle_alloc_error(new_layout);
    }

    /// Sets all elements in the allocated memory space to the default value of `T`.
    ///
    /// Indexing is zero-based, i.e., the last element is at offset `count - 1`, this will make
    /// the writing range `[0, count - 1]`.
    ///
    /// # Safety
    ///
    /// - Pointer must be allocated before calling this method.
    ///   Calling this method with a null pointer will cause termination with `SIGABRT`.
    ///
    /// - `count` must be within the bounds of the allocated memory space.
    ///
    /// - Initialized elements will be overwritten **without** calling `drop`.
    ///   This might cause memory leaks if `T` is not of trivial type, or if the elements are not
    ///   dropped properly before calling this method.
    ///
    /// # Time Complexity
    ///
    /// _O_(n) where `n` is allocated count of type `T`.
    ///
    #[inline(always)]
    pub(crate) unsafe fn memset_default(&mut self, count: usize)
    where
        T: Default,
    {
        #[cfg(debug_assertions)]
        debug_assert_allocated(self);

        // Write the value to all elements
        for i in 0..count {
            ptr::write((self.ptr as *mut T).add(i), T::default());
        }
    }

    /// Stores a value at the specified offset `at`.
    ///
    /// # Safety
    ///
    /// - Pointer must be allocated before calling this method.
    ///   Calling this method with a null pointer will cause termination with `SIGABRT`.
    ///
    /// - `at` must be within the bounds of the allocated memory space.
    ///
    /// - If the offset has already been initialized, the value will be overwritten **without**
    ///   calling `drop`. This might cause memory leaks if the element is not of trivial type,
    ///   or not dropped properly before overwriting.
    ///
    /// # Time Complexity
    ///
    /// _O_(1).
    ///
    #[inline(always)]
    pub(crate) const unsafe fn store(&mut self, at: usize, value: T) {
        #[cfg(debug_assertions)]
        debug_assert_allocated(self);

        ptr::write((self.ptr as *mut T).add(at), value);
    }

    /// Returns a reference to an initialized element at the specified offset `at`.
    ///
    /// # Safety
    ///
    /// - Pointer must be allocated before calling this method.
    ///   Calling this method with a null pointer will cause termination with `SIGSEGV`.
    ///
    /// - `at` must be within the bounds of the initialized elements.
    ///   Loading an uninitialized elements as `T` is `undefined behavior`.
    ///
    /// # Time Complexity
    ///
    /// _O_(1).
    ///
    #[must_use]
    #[inline(always)]
    pub(crate) const unsafe fn load(&self, at: usize) -> &T {
        #[cfg(debug_assertions)]
        debug_assert_allocated(self);

        &*self.ptr.add(at)
    }

    /// Returns a mutable reference to an initialized element at the specified offset `at`.
    ///
    /// # Safety
    ///
    /// - Pointer must be allocated before calling this method.
    ///   Calling this method with a null pointer will cause termination with `SIGSEGV`.
    ///
    /// - `at` must be within the bounds of the initialized elements.
    ///   Loading an uninitialized elements as `T` is `undefined behavior`.
    ///
    /// # Time Complexity
    ///
    /// _O_(1).
    ///
    #[must_use]
    #[inline(always)]
    pub(crate) const unsafe fn load_mut(&mut self, at: usize) -> &mut T {
        #[cfg(debug_assertions)]
        debug_assert_allocated(self);

        &mut *(self.ptr as *mut T).add(at)
    }

    /// Returns a reference to the first initialized element.
    ///
    /// # Safety
    ///
    /// This method checks for out of bounds access in debug mode only.
    ///
    /// The caller must ensure that the `UnsafeBufferPointer` is not empty.
    ///
    /// # Time Complexity
    ///
    /// _O_(1).
    ///
    #[must_use]
    #[inline(always)]
    pub(crate) const unsafe fn load_first(&self) -> &T {
        #[cfg(debug_assertions)]
        debug_assert_allocated(self);

        &*self.ptr
    }

    /// Removes and returns the initialized element at the specified offset `at`, and shifts the
    /// `count` values after `at` to fill the gap.
    ///
    /// # Safety
    ///
    /// - Pointer must be allocated before calling this method.
    ///   Calling this method with a null pointer will cause termination with `SIGABRT`.
    ///
    /// - `at + count` must be within the bounds of the initialized elements.
    ///   Loading an uninitialized elements as `T` is `undefined behavior`.
    ///
    /// # Time Complexity
    ///
    /// _O_(n) where `n` is the number (`count`) of the elements to be shifted.
    ///
    pub(crate) const unsafe fn take_shift_left(&mut self, at: usize, count: usize) -> T {
        #[cfg(debug_assertions)]
        debug_assert_allocated(self);

        // infallible
        let value;
        {
            // The source offset.
            let src = (self.ptr as *mut T).add(at);

            // The destination offset.
            let dst = src.add(1);

            // Copy value to the stack.
            value = ptr::read(src);

            // Shift everything down to fill in.
            ptr::copy(dst, src, count);
        }

        // Return the removed value.
        // The stack is now responsible for dropping the value.
        value
    }

    /// Removes and returns the initialized element at the specified offset `at`.
    ///
    /// This method does not shift the elements to fill in the gap, all elements after the offset
    /// will remain in place.
    ///
    /// # Safety
    ///
    /// - Pointer must be allocated before calling this method.
    ///   Calling this method with a null pointer will cause termination with `SIGABRT`.
    ///
    /// - `at` must be within the bounds of the initialized elements.
    ///   Loading an uninitialized elements as `T` is `undefined behavior`.
    ///
    /// # Time Complexity
    ///
    /// _O_(1).
    ///
    #[inline(always)]
    pub(crate) const unsafe fn take_no_shift(&mut self, at: usize) -> T {
        #[cfg(debug_assertions)]
        debug_assert_allocated(self);

        // Return the removed value.
        // The stack is now responsible for dropping the value.
        ptr::read((self.ptr as *mut T).add(at))
    }

    /// Calls `drop` on the initialized elements with the specified `count` starting from the
    /// offset `0`.
    ///
    /// Indexing is zero-based, i.e., the last element is at offset `count - 1`, this will make
    /// the drop range `[0, count - 1]`.
    ///
    /// This method is no-op when `count` is `0` or when `T` is of trivial type.
    ///
    /// # Safety
    ///
    /// - Pointer must be allocated before calling this method.
    ///   Calling this method with a null pointer will cause termination with `SIGABRT`.
    ///
    /// - `count` must be within the bounds of the **initialized** elements.
    ///   Calling `drop` on uninitialized elements is `undefined behavior`.
    ///
    /// - If `T` is not of trivial type, using dropped values after calling this method can cause
    ///   `undefined behavior`.
    ///
    /// # Time Complexity
    ///
    /// _O_(n) where `n` is the number (`count`) of the elements to be dropped.
    ///
    #[inline(always)]
    pub(crate) unsafe fn drop_initialized(&mut self, count: usize) {
        #[cfg(debug_assertions)]
        debug_assert_allocated(self);

        // Call drop on each element to release resources.
        ptr::drop_in_place(std::slice::from_raw_parts_mut(self.ptr as *mut T, count));
    }

    /// Calls `drop` on the initialized elements in the specified range.
    ///
    /// The total drop `count` equals `end - start - 1`.
    ///
    /// This method is no-op when `T` is of trivial type.
    ///
    /// # Safety
    ///
    /// - Pointer must be allocated before calling this method.
    ///   Calling this method with a null pointer will cause termination with `SIGABRT`.
    ///
    /// - `range` must not be empty.
    ///
    /// - `range` must be within the bounds of the **initialized** elements.
    ///   Calling `drop` on uninitialized elements is `undefined behavior`.
    ///
    /// - If `T` is not of trivial type, using dropped values after calling this method can cause
    ///   `undefined behavior`.
    ///
    /// These invariants are checked in debug mode only.
    ///
    /// # Time Complexity
    ///
    /// _O_(n) where `n` is the number (`count`) of the elements to be dropped.
    ///
    #[inline(always)]
    pub(crate) unsafe fn drop_range(&mut self, range: Range<usize>) {
        #[cfg(debug_assertions)]
        debug_assert_allocated(self);

        debug_assert!(!range.is_empty(), "Drop range must not be empty");

        // Call drop on each element to release resources.
        ptr::drop_in_place(std::slice::from_raw_parts_mut(
            self.ptr.add(range.start) as *mut T,
            range.end - range.start,
        ));
    }

    /// Returns an immutable slice of the initialized elements starting from the offset `0`.
    ///
    /// Indexing is zero-based, i.e., the last element is at offset `count - 1`, this will make
    /// the slice range `[0, count - 1]`.
    ///
    /// # Safety
    ///
    /// - Pointer must be allocated before calling this method.
    ///   Calling this method with a null pointer will cause termination with `SIGABRT`.
    ///
    /// - `count` must be within the bounds of the initialized elements.
    ///   Loading an uninitialized elements as `T` is `undefined behavior`.
    ///
    /// # Time Complexity
    ///
    /// _O_(1).
    ///
    pub(crate) const unsafe fn as_slice(&self, count: usize) -> &[T] {
        #[cfg(debug_assertions)]
        debug_assert_allocated(self);

        std::slice::from_raw_parts(self.ptr, count)
    }

    /// Returns a mutable slice over `count` initialized elements starting from the offset `0`.
    ///
    /// Indexing is zero-based, i.e., the last element is at offset `count - 1`, this will make
    /// the slice range `[0, count - 1]`.
    ///
    /// # Safety
    ///
    /// - Pointer must be allocated before calling this method.
    ///   Calling this method with a null pointer will cause termination with `SIGABRT`.
    ///
    /// - `count` must be within the bounds of the initialized elements.
    ///   Loading an uninitialized elements as `T` is `undefined behavior`.
    ///
    /// # Time Complexity
    ///
    /// _O_(1).
    ///
    pub(crate) const unsafe fn as_slice_mut(&mut self, count: usize) -> &mut [T] {
        #[cfg(debug_assertions)]
        debug_assert_allocated(self);

        std::slice::from_raw_parts_mut(self.ptr as *mut T, count)
    }
}

impl<T: Clone> UnsafeBufferPointer<T> {
    /// Makes a clone of the `UnsafeBufferPointer` with the `allocation_count` as memory space and
    /// `clone_count` as the number of initialized elements to clone.
    ///
    /// # Safety
    ///
    /// - Pointer must be allocated before calling this method.
    ///   Calling this method with a null pointer will cause termination with `SIGABRT`.
    ///
    /// - `allocation_count` must be greater than `0`.
    ///
    /// - `clone_count` must less than or equal to `allocation_count`.
    ///
    /// - `clone_count` must be within the bounds of the initialized elements.
    ///   Cloning an uninitialized elements as `T` is `undefined behavior`.
    ///
    #[must_use]
    pub(crate) unsafe fn make_clone(&self, allocation_count: usize, clone_count: usize) -> Self {
        #[cfg(debug_assertions)]
        debug_assert_allocated(self);

        #[cfg(debug_assertions)]
        debug_assert_copy_inbounds(allocation_count, clone_count);

        // New instance.
        let cloned_buffer = Self::new_allocate(allocation_count);

        // Clone initialized elements.
        unsafe {
            for i in 0..clone_count {
                let src = self.ptr.add(i);
                let dst = (cloned_buffer.ptr as *mut T).add(i);
                ptr::write(dst, (*src).clone());
            }
        }

        // Done.
        cloned_buffer
    }
}

/// `UnsafeBufferPointer` can't meaningfully implement `Drop` trait, as it doesn't store any
/// metadata about the allocated memory space.
///
/// This implementation is a debug-mode check to ensure that the allocated memory space is
/// deallocated before dropping the `UnsafeBufferPointer`.
#[cfg(debug_assertions)]
impl<T> Drop for UnsafeBufferPointer<T> {
    fn drop(&mut self) {
        // The `drop` method is called automatically when the thread is panicking.
        // If the thread is panicking, this check is skipped to avoid double panic.
        if !std::thread::panicking() {
            assert!(
                self.ptr.is_null(),
                "Pointer must be deallocated before dropping"
            );
        }
    }
}

#[cfg(test)]
mod ptr_tests {
    use super::*;

    #[test]
    fn test_buffer_ptr_new() {
        let buffer_ptr: UnsafeBufferPointer<u8> = UnsafeBufferPointer::new();
        assert!(buffer_ptr.is_null());
    }

    #[test]
    fn test_buffer_ptr_new_allocate() {
        unsafe {
            let mut buffer_ptr: UnsafeBufferPointer<u8> = UnsafeBufferPointer::new_allocate(3);

            // Memory space should have been allocated.
            assert!(!buffer_ptr.is_null());

            // Deallocate memory space.
            buffer_ptr.deallocate(3);
        }
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Allocation size must be greater than 0")]
    fn test_buffer_ptr_new_allocate_zero_size() {
        // count is 0, should panic.
        let _: UnsafeBufferPointer<u8> = unsafe { UnsafeBufferPointer::new_allocate(0) };
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Allocation size exceeds maximum limit on this platform")]
    fn test_buffer_ptr_new_allocate_overflow() {
        let _: UnsafeBufferPointer<u8> =
            unsafe { UnsafeBufferPointer::new_allocate(isize::MAX as usize + 1) };
    }

    #[test]
    fn test_buffer_ptr_allocate() {
        let mut buffer_ptr: UnsafeBufferPointer<u8> = UnsafeBufferPointer::new();

        // Allocate memory space.
        unsafe {
            buffer_ptr.allocate(3);

            assert!(!buffer_ptr.is_null());

            buffer_ptr.deallocate(3);
        }
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Allocation size must be greater than 0")]
    fn test_buffer_ptr_allocate_zero_size() {
        let mut buffer_ptr: UnsafeBufferPointer<u8> = UnsafeBufferPointer::new();

        // count must be greater than 0, should panic.
        unsafe {
            buffer_ptr.allocate(0);
        }
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Allocation size exceeds maximum limit on this platform")]
    fn test_buffer_ptr_allocate_overflow() {
        let mut buffer_ptr: UnsafeBufferPointer<u8> = UnsafeBufferPointer::new();

        // Size exceeds maximum limit, should panic.
        unsafe {
            buffer_ptr.allocate(isize::MAX as usize + 1);
        }
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Pointer must be null")]
    fn test_buffer_ptr_allocate_allocated() {
        let mut buffer_ptr: UnsafeBufferPointer<u8> = UnsafeBufferPointer::new();
        unsafe {
            // Not yet allocated, should not panic.
            buffer_ptr.allocate(1);

            assert!(!buffer_ptr.is_null());

            // Already allocated, should panic.
            buffer_ptr.allocate(2);
        }
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
        unsafe {
            let mut buffer_ptr: UnsafeBufferPointer<Choice> = UnsafeBufferPointer::new_allocate(3);

            // Set all elements to the default value of `Choice`.
            buffer_ptr.memset_default(3);

            // Values were uninit, so they should be set to `Default`.
            for i in 0..3 {
                assert!(matches!(*buffer_ptr.ptr.add(i), Choice::Default))
            }

            // Deallocate memory space.
            buffer_ptr.deallocate(3);
        }
    }

    #[test]
    fn test_buffer_ptr_new_allocate_default() {
        unsafe {
            let mut buffer_ptr: UnsafeBufferPointer<Choice> =
                UnsafeBufferPointer::new_allocate_default(3);

            // Memory space should have been allocated.
            assert!(!buffer_ptr.is_null());

            // All elements are must have been initialized to their default values.
            for i in 0..3 {
                assert!(matches!(*buffer_ptr.ptr.add(i), Choice::Default))
            }

            // Deallocate memory space.
            buffer_ptr.deallocate(3);
        }
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Allocation size must be greater than 0")]
    fn test_buffer_ptr_new_allocate_default_zero_count() {
        let _: UnsafeBufferPointer<u8> = unsafe { UnsafeBufferPointer::new_allocate_default(0) };

        // count is 0, no allocation should have been made.
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Allocation size exceeds maximum limit on this platform")]
    fn test_buffer_ptr_new_allocate_default_overflow() {
        let _: UnsafeBufferPointer<u8> =
            unsafe { UnsafeBufferPointer::new_allocate_default(isize::MAX as usize + 1) };
    }

    #[test]
    fn test_buffer_ptr_reallocate() {
        unsafe {
            let mut buffer_ptr: UnsafeBufferPointer<u8> = UnsafeBufferPointer::new_allocate(3);

            *(buffer_ptr.ptr as *mut u8) = 1;
            *(buffer_ptr.ptr as *mut u8).add(1) = 2;
            *(buffer_ptr.ptr as *mut u8).add(2) = 3;

            // Grows the count to 5.
            buffer_ptr.reallocate(3, 5, 3);

            // Check values after reallocation.
            for i in 0..3 {
                assert_eq!(*buffer_ptr.ptr.add(i), i as u8 + 1);
            }

            // Deallocate memory space.
            buffer_ptr.deallocate(3);
        }
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Pointer must not be null")]
    fn test_buffer_ptr_reallocate_null_ptr() {
        let mut buffer_ptr: UnsafeBufferPointer<u8> = UnsafeBufferPointer::new();

        // Not yet allocated, should panic.
        unsafe {
            buffer_ptr.reallocate(5, 10, 5);
        }
    }

    #[test]
    fn test_buffer_ptr_store_load() {
        unsafe {
            let mut buffer_ptr: UnsafeBufferPointer<u8> = UnsafeBufferPointer::new_allocate(3);

            // Store some values.
            for i in 0..3 {
                buffer_ptr.store(i, i as u8 + 1);
            }

            assert_eq!(*buffer_ptr.load(0), 1);
            assert_eq!(*buffer_ptr.load(1), 2);
            assert_eq!(*buffer_ptr.load(2), 3);

            // Deallocate memory space.
            buffer_ptr.deallocate(3);
        }
    }

    #[test]
    fn test_buffer_ptr_load_mut() {
        unsafe {
            let mut buffer_ptr: UnsafeBufferPointer<u8> = UnsafeBufferPointer::new_allocate(3);

            // Store some values.
            buffer_ptr.store(0, 1);
            buffer_ptr.store(1, 2);

            // Mutate the value.
            *buffer_ptr.load_mut(0) = 10;

            // Value should be updated.
            assert_eq!(*buffer_ptr.load(0), 10);

            // Deallocate memory space.
            buffer_ptr.deallocate(3);
        }
    }

    #[test]
    fn test_buffer_ptr_load_first() {
        unsafe {
            let mut buffer_ptr: UnsafeBufferPointer<u8> = UnsafeBufferPointer::new_allocate(3);
            buffer_ptr.store(0, 1);
            buffer_ptr.store(1, 2);

            assert_eq!(buffer_ptr.load_first(), &1);

            // Deallocate memory space.
            buffer_ptr.deallocate(3);
        }
    }

    #[test]
    fn test_buffer_ptr_take_shift() {
        unsafe {
            let mut buffer_ptr: UnsafeBufferPointer<u8> = UnsafeBufferPointer::new_allocate(3);

            // Store some values.
            buffer_ptr.store(0, 1);
            buffer_ptr.store(1, 2);

            // Take the value and shift the elements after the offset to fill the gap.
            assert_eq!(buffer_ptr.take_shift_left(0, 2), 1);

            // Value should be removed and the next value should be at the offset 0.
            assert_eq!(*buffer_ptr.load(0), 2);

            // Deallocate memory space.
            buffer_ptr.deallocate(3);
        }
    }

    #[test]
    fn test_buffer_ptr_take_no_shift() {
        unsafe {
            let mut buffer_ptr: UnsafeBufferPointer<u8> = UnsafeBufferPointer::new_allocate(3);

            // Store some values.
            buffer_ptr.store(0, 1);
            buffer_ptr.store(1, 2);

            // Take the first value without shifting the elements.
            assert_eq!(buffer_ptr.take_no_shift(0), 1);

            // The next value should remain at the offset 1.
            assert_eq!(*buffer_ptr.load(1), 2);

            // Deallocate memory space.
            buffer_ptr.deallocate(3);
        }
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Pointer must not be null")]
    fn test_buffer_ptr_as_slice_null_ptr() {
        let buffer_ptr: UnsafeBufferPointer<u8> = UnsafeBufferPointer::new();
        let slice = unsafe { buffer_ptr.as_slice(0) };
        assert_eq!(slice, &[]);
    }

    #[test]
    fn test_buffer_ptr_as_slice_empty() {
        unsafe {
            let mut buffer_ptr: UnsafeBufferPointer<u8> = UnsafeBufferPointer::new_allocate(3);
            let slice = buffer_ptr.as_slice(0);
            assert_eq!(slice, &[]);

            // Deallocate memory space or the destructor will panic.
            buffer_ptr.deallocate(3);
        }
    }

    #[test]
    fn test_buffer_ptr_as_slice() {
        unsafe {
            let mut buffer_ptr: UnsafeBufferPointer<u8> = UnsafeBufferPointer::new_allocate(3);

            // Store some values.
            for i in 0..3 {
                buffer_ptr.store(i, i as u8 + 1);
            }

            // Values should be accessible as a slice.
            let slice = buffer_ptr.as_slice(3);
            assert_eq!(slice, &[1, 2, 3]);

            // Deallocate memory space.
            buffer_ptr.deallocate(3);
        }
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Pointer must not be null")]
    fn test_buffer_ptr_as_slice_mut_null_ptr() {
        let mut buffer_ptr: UnsafeBufferPointer<u8> = UnsafeBufferPointer::new();
        let slice = unsafe { buffer_ptr.as_slice_mut(0) };
        assert_eq!(slice, &mut []);
    }

    #[test]
    fn test_buffer_ptr_as_slice_mut_empty() {
        unsafe {
            let mut buffer_ptr: UnsafeBufferPointer<u8> = UnsafeBufferPointer::new_allocate(3);
            let slice = buffer_ptr.as_slice_mut(0);
            assert_eq!(slice, &[]);

            // Deallocate memory space or the destructor will panic.
            buffer_ptr.deallocate(3);
        }
    }

    #[test]
    fn test_buffer_ptr_as_slice_mut() {
        unsafe {
            let mut buffer_ptr: UnsafeBufferPointer<u8> = UnsafeBufferPointer::new_allocate(3);
            // Store some values.
            for i in 0..3 {
                buffer_ptr.store(i, i as u8 + 1);
            }

            // Values should be accessible as a mutable slice.
            let slice = buffer_ptr.as_slice_mut(3);
            assert_eq!(slice, &mut [1, 2, 3]);

            // Deallocate memory space.
            buffer_ptr.deallocate(3);
        }
    }

    use std::cell::RefCell;
    use std::rc::Rc;

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

    #[test]
    fn test_drop_values() {
        // Drop counter with 0 count initially.
        let drop_count = Rc::new(RefCell::new(0));

        unsafe {
            let mut buffer_ptr: UnsafeBufferPointer<DropCounter> =
                UnsafeBufferPointer::new_allocate(3);

            // Reference 5 elements to the same drop counter.
            for i in 0..3 {
                buffer_ptr.store(
                    i,
                    DropCounter {
                        count: Rc::clone(&drop_count),
                    },
                );
            }

            // Dropping with count 0 is a no-op.
            buffer_ptr.drop_initialized(0);
            assert_eq!(*drop_count.borrow(), 0);

            // Drop all elements.
            buffer_ptr.drop_initialized(3);

            // Since the `drop` has been called on all elements, the drop count must be 3.
            assert_eq!(*drop_count.borrow(), 3);

            // Deallocate memory space.
            buffer_ptr.deallocate(3);
        }
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Drop range must not be empty")]
    fn test_buffer_ptr_drop_range_invalid() {
        unsafe {
            let mut buffer_ptr: UnsafeBufferPointer<u8> = UnsafeBufferPointer::new_allocate(5);
            buffer_ptr.drop_range(0..0);
        }
    }

    #[test]
    fn test_buffer_ptr_drop_range() {
        // Drop counter with 0 count initially.
        let drop_count = Rc::new(RefCell::new(0));

        unsafe {
            let mut buffer_ptr: UnsafeBufferPointer<DropCounter> =
                UnsafeBufferPointer::new_allocate(5);

            // Reference 5 elements to the same drop counter.
            for i in 0..5 {
                buffer_ptr.store(
                    i,
                    DropCounter {
                        count: Rc::clone(&drop_count),
                    },
                );
            }

            // Drop 3 elements in the range [0, 3 - 1].
            buffer_ptr.drop_range(0..3);

            // Since the `drop` has been called on 3 elements, the drop count must be 3.
            assert_eq!(*drop_count.borrow(), 3);

            // Deallocate memory space.
            buffer_ptr.deallocate(5);
        }
    }

    #[test]
    fn test_buffer_ptr_drop_values() {
        // Drop counter with 0 count initially.
        let drop_count = Rc::new(RefCell::new(0));

        unsafe {
            let mut buffer_ptr: UnsafeBufferPointer<DropCounter> =
                UnsafeBufferPointer::new_allocate(3);

            // Reference 3 elements to the same drop counter.
            for i in 0..3 {
                buffer_ptr.store(
                    i,
                    DropCounter {
                        count: Rc::clone(&drop_count),
                    },
                );
            }

            // Drop initialized elements.
            buffer_ptr.drop_initialized(3);

            // Since the `drop` has been called, pointer should have called drop on all elements,
            // so the drop count must be 3.
            assert_eq!(*drop_count.borrow(), 3);

            // Deallocate memory space.
            buffer_ptr.deallocate(3);
        }
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Pointer must not be null")]
    fn test_buffer_ptr_clone_empty() {
        let original: UnsafeBufferPointer<u8> = UnsafeBufferPointer::new();

        // Cloning an empty pointer should panic.
        let _ = unsafe { original.make_clone(0, 0) };
    }

    #[test]
    fn test_buffer_ptr_clone() {
        unsafe {
            // Box is non-trivial type.
            let mut original: UnsafeBufferPointer<Box<u8>> = UnsafeBufferPointer::new_allocate(5);

            for i in 0..3 {
                original.store(i, Box::new(i as u8 + 1));
            }

            // Clone without compacting.
            let mut cloned = original.make_clone(5, 3);

            // Cloned must have different pointers.
            assert_ne!(cloned.ptr.addr(), original.ptr.addr());

            // The elements in the clone must have the same values as in the original.
            for i in 0..3 {
                assert_eq!(cloned.load(i), original.load(i));
            }

            // Allocate more memory for the clone.
            cloned.reallocate(5, 6, 3);

            // Add a new element to the clone.
            cloned.store(3, Box::new(4));

            // Mutating the original must not affect the clone.
            let first_origin = original.load_mut(0);
            **first_origin = 10;

            // The original must have the new value.
            assert_eq!(**original.load(0), 10);

            // The cloned must not be affected.
            assert_eq!(**cloned.load(0), 1);

            // Mutating the clone must not affect the original.
            let first_cloned = cloned.load_mut(0);
            **first_cloned = 11;

            // The original must not be affected.
            assert_eq!(**original.load(0), 10);

            // The cloned must have the new value.
            assert_eq!(**cloned.load(0), 11);

            // Drop the elements in the original and deallocate memory space.
            original.drop_initialized(3);
            original.deallocate(5);

            // Drop the elements in the clone and deallocate memory space.
            cloned.drop_initialized(4);
            cloned.deallocate(6);
        }
    }

    #[test]
    fn test_buffer_ptr_clone_compact() {
        unsafe {
            // Box is non-trivial type.
            let mut original: UnsafeBufferPointer<Box<u8>> = UnsafeBufferPointer::new_allocate(5);

            for i in 0..3 {
                original.store(i, Box::new(i as u8 + 1));
            }

            // Clone with the length as the count.
            let mut cloned = original.make_clone(3, 3);

            // Cloned must have different pointers.
            assert_ne!(cloned.ptr.addr(), original.ptr.addr());

            // The elements in the clone must have the same values as in the original.
            for i in 0..3 {
                assert_eq!(cloned.load(i), original.load(i));
            }

            // Reallocate memory for the clone.
            cloned.reallocate(3, 6, 3);

            // Add a new element to the clone.
            cloned.store(3, Box::new(4));

            // Mutating the original must not affect the clone.
            let first_origin = original.load_mut(0);
            **first_origin = 10;

            // The original must have the new value.
            assert_eq!(**original.load(0), 10);

            // The cloned must not be affected.
            assert_eq!(**cloned.load(0), 1);

            // Mutating the clone must not affect the original.
            let first_cloned = cloned.load_mut(0);
            **first_cloned = 11;

            // The original must not be affected.
            assert_eq!(**original.load(0), 10);

            // The cloned must have the new value.
            assert_eq!(**cloned.load(0), 11);

            // Drop the elements in the original and deallocate memory space.
            original.drop_initialized(3);
            original.deallocate(5);

            // Drop the elements in the clone and deallocate memory space.
            cloned.drop_initialized(4);
            cloned.deallocate(6);
        }
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Pointer must be deallocated before dropping")]
    fn test_buffer_ptr_drop() {
        let _: UnsafeBufferPointer<u8> = unsafe { UnsafeBufferPointer::new_allocate(1) };

        // Dropping the pointer without deallocating the memory space should panic.
    }
}
