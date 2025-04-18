use core::alloc::Layout;
use core::hint::unreachable_unchecked;
use core::mem::ManuallyDrop;

use crate::alloc::UnsafeBufferPointer;
use crate::error::OnError;
use crate::AllocError;

/// The state of the slot in the index.
#[derive(Clone, Copy, Debug)]
#[repr(u8)]
pub(crate) enum Tag {
    Empty = 0,
    Deleted = 1,
    Occupied = 2,
}

impl Tag {
    #[inline(always)]
    pub(crate) const fn is_empty(self) -> bool {
        self as u8 == Tag::Empty as u8
    }

    #[inline(always)]
    pub(crate) const fn is_deleted(self) -> bool {
        self as u8 == Tag::Deleted as u8
    }

    #[inline(always)]
    pub(crate) const fn is_occupied(self) -> bool {
        self as u8 == Tag::Occupied as u8
    }
}

/// A helper type to manage index's memory.
pub(crate) struct MapIndex {
    // Figure 1:
    // -----------------------------------------------------------------------------------
    // |                        The memory layout of the index                           |
    // | |------------ usize * N ------------|---------- u8 * N ------------|- u8 * X -| |
    // | [ IndexN-1 | ... | Index1 | Index0 ]|[ Tag0 | Tag1 | ... | TagN-1 ] [ Padding ] |
    // |                usize strides (-) <- ^ -> (+) u8 strides                         |
    // |                                     |                                           |
    // |                                  Pointer                                        |
    // | Legend:                                                                         |
    // | N: The allocated capacity.                                                      |
    // | X: The required bytes to round the total size to multiple of usize's alignment. |
    // | Tag: A single byte as flag to store slot's state.                               |
    // | Index: A usize-value that stores an offset where an entry can be located.       |
    // -----------------------------------------------------------------------------------
    pub(crate) pointer: UnsafeBufferPointer<u8>,
}

impl MapIndex {
    const T_SIZE: usize = size_of::<usize>();
    const T_ALIGN: usize = align_of::<usize>();
    const T_MAX_ALLOC_SIZE: usize = (isize::MAX as usize + 1) - Self::T_ALIGN;

    /// Returns the `(aligned layout, slots size)` of the index for a given capacity `cap`.
    /// Size and alignment are calculated for `usize`.
    ///
    /// This function checks for overflow and valid layout's size.
    #[inline]
    fn index_layout(cap: usize) -> Option<(Layout, usize)> {
        let slots_size = cap.checked_mul(Self::T_SIZE)?;
        let aligned_tags = cap.checked_add(Self::T_ALIGN - 1)? & !(Self::T_ALIGN - 1);
        let total_size = slots_size.checked_add(aligned_tags)?;
        if Self::T_MAX_ALLOC_SIZE > total_size {
            // This layout thing is just awful...
            let layout = unsafe { Layout::from_size_align_unchecked(total_size, Self::T_ALIGN) };
            return Some((layout, slots_size));
        }
        None
    }

    /// Checks if the index's pointer is null.
    #[inline(always)]
    pub(crate) const fn not_allocated(&self) -> bool {
        self.pointer.is_null()
    }

    /// Creates new unallocated index.
    #[inline(always)]
    pub(crate) const fn new_unallocated() -> Self {
        Self {
            pointer: UnsafeBufferPointer::new(),
        }
    }

    /// Allocates the buffer with capacity `cap`, without initializing control tags.
    ///
    /// Handling errors will be done according to the passed error context `on_err`.
    #[inline]
    pub(crate) fn allocate_uninit(
        &mut self,
        cap: usize,
        on_err: OnError,
    ) -> Result<(), AllocError> {
        unsafe {
            let mut pointer = UnsafeBufferPointer::new();
            match Self::index_layout(cap) {
                Some((layout, slots_size)) => {
                    pointer.allocate(layout, on_err)?;
                    // Set the pointer at the offset of the control tags.
                    pointer.set_plus(slots_size);
                    self.pointer = pointer;
                    Ok(())
                }
                None => Err(on_err.overflow()),
            }
        }
    }

    /// Copies bitwise `cap` count values from `source` to `self`.
    ///
    /// # Safety
    ///
    /// - This instance and `source` must be allocated before calling this method.
    /// - `cap` must be the same allocated capacity by both in order to copy data correctly.
    #[inline]
    pub(crate) const unsafe fn copy_from(&mut self, source: &MapIndex, cap: usize) {
        let slots_size = cap * Self::T_SIZE;
        // Copy the useful data without the padding bytes.
        let unaligned_size = slots_size + cap;
        unsafe {
            let source_start = source.pointer.pointer().sub(slots_size);
            let target_start = self.pointer.pointer().sub(slots_size);
            core::ptr::copy_nonoverlapping(source_start, target_start as *mut u8, unaligned_size)
        }
    }

    /// Resets the pointer to the start of the allocated buffer and deallocates the current index
    /// according to the current capacity.
    ///
    /// # Safety
    ///
    /// - Index must be allocated before calling this method.
    /// - `cap` must be the same allocated capacity.
    #[inline]
    pub(crate) unsafe fn deallocate(&mut self, cap: usize) {
        debug_assert!(!self.pointer.is_null());
        unsafe {
            match Self::index_layout(cap) {
                Some((layout, slots_size)) => {
                    // Reset the pointer to the start of the allocated memory.
                    self.pointer.set_minus(slots_size);
                    self.pointer.deallocate(layout)
                }
                // Already checked when allocated, so it must not fail.
                None => unreachable_unchecked(),
            }
        }
    }

    /// Reads and returns the control tag in the index at tag's `offset`.
    ///
    /// # Safety
    ///
    /// - Index must be allocated and control tags must be initialized before calling this method.
    ///
    /// - Safe casting to `Tag` depends on zeroing tags when allocating and reallocating and
    ///   using `Tag` enum to store tag's value.
    #[inline(always)]
    pub(crate) const unsafe fn read_tag(&self, offset: usize) -> Tag {
        unsafe { self.pointer.pointer_as::<Tag>().add(offset).read() }
    }

    /// Stores the control tag at the specified tag's `offset`.
    ///
    /// # Safety
    ///
    /// Index must be allocated before calling this method.
    #[inline(always)]
    pub(crate) const unsafe fn store_tag(&mut self, offset: usize, tag: Tag) {
        unsafe {
            self.pointer.store(offset, tag as u8);
        }
    }

    /// Returns a mutable reference to the control tag in the index at tag's `offset`.
    ///
    /// # Safety
    ///
    /// - Index must be allocated and control tags must be initialized before calling this method.
    ///
    /// - Safe casting to `Tag` depends on zeroing tags when allocating and reallocating and
    ///   using `Tag` enum to store tag's value.
    #[inline(always)]
    pub(crate) const unsafe fn tag_ref_mut(&mut self, offset: usize) -> &mut Tag {
        unsafe { &mut *self.pointer.pointer_mut_as::<Tag>().add(offset) }
    }

    /// Reads and returns the slot's value according to the specified tag's `offset`.
    ///
    /// # Safety
    ///
    /// Index must be allocated before calling this method.
    #[inline(always)]
    pub(crate) const unsafe fn read_entry_index(&self, offset: usize) -> usize {
        unsafe { self.pointer.pointer_as::<usize>().sub(offset + 1).read() }
    }

    /// Stores slot's value according to the specified tag's `offset`.
    ///
    /// # Safety
    ///
    /// Index must be allocated before calling this method.
    #[inline(always)]
    pub(crate) const unsafe fn store_entry_index(&mut self, offset: usize, value: usize) {
        unsafe {
            self.pointer
                .pointer_mut_as::<usize>()
                .sub(offset + 1)
                .write(value)
        }
    }

    /// Returns a mutable reference to a slot's value according to the specified tag's `offset`.
    ///
    /// # Safety
    ///
    /// Index must be allocated before calling this method.
    #[inline(always)]
    pub(crate) const unsafe fn entry_index_ref_mut(&mut self, offset: usize) -> &mut usize {
        unsafe { &mut *self.pointer.pointer_mut_as::<usize>().sub(offset + 1) }
    }

    /// Stores the control tag and slot's value at the specified tag's `offset`.
    ///
    /// # Safety
    ///
    /// Index must be allocated before calling this method.
    #[inline(always)]
    pub(crate) const unsafe fn store(&mut self, offset: usize, tag: Tag, value: usize) {
        self.store_tag(offset, tag);
        self.store_entry_index(offset, value);
    }

    /// Sets all control tags to empty.
    ///
    /// # Safety
    ///
    /// Index must be allocated before calling this method.
    #[inline(always)]
    pub(crate) const unsafe fn set_tags_empty(&mut self, cap: usize) {
        unsafe { self.pointer.memset_zero(cap) }
    }

    /// Returns scope guard, that ensure deallocating index in case of a sudden divergence from
    /// normal execution before deactivating the guard.
    ///
    /// A Guard instance is safe to be created even if the index is still unallocated.
    ///
    /// # Safety
    ///
    /// - `cap` must be the same capacity used to allocate index.
    ///
    /// - The returned guard must not outlive the guarded index instance.
    ///
    /// - At the end of guarding scope, `deactivate()` must be called to deactivate the guard.
    #[inline(always)]
    pub(crate) const unsafe fn guard(&self, cap: usize) -> IndexGuard {
        IndexGuard {
            index: self as *const _ as *mut MapIndex,
            cap,
        }
    }
}

pub(crate) struct IndexGuard {
    index: *mut MapIndex,
    cap: usize,
}

impl IndexGuard {
    /// Deactivates the guard of the index instance to be manually deallocated again.
    #[inline(always)]
    pub(crate) const fn deactivate(self) {
        let _ = ManuallyDrop::new(self);
    }
}

impl Drop for IndexGuard {
    fn drop(&mut self) {
        unsafe {
            if !(*self.index).pointer.is_null() {
                (*self.index).deallocate(self.cap);
            }
        }
    }
}

#[cfg(test)]
mod index_tests {
    use super::*;

    #[test]
    fn test_index_new() {
        let instance = MapIndex::new_unallocated();
        assert!(instance.pointer.is_null());
    }

    #[test]
    fn test_index_layout() {
        let (layout, slots_size) = MapIndex::index_layout(10).unwrap();

        assert_eq!(layout.align(), 8);

        assert_eq!(slots_size, 80);

        // 80 bytes for slots and 10 control tags and 6 padding bytes.
        assert_eq!(layout.size(), 96);
    }

    #[test]
    fn test_index_allocate_uninitialized() {
        let mut instance = MapIndex::new_unallocated();
        assert!(instance.pointer.is_null());

        let result = instance.allocate_uninit(10, OnError::NoReturn);
        assert!(result.is_ok());
        assert!(!instance.pointer.is_null());

        unsafe { instance.deallocate(10) }
    }

    #[test]
    fn test_index_allocate_uninitialized_error() {
        let mut instance = MapIndex::new_unallocated();
        assert!(instance.pointer.is_null());

        let result = instance.allocate_uninit(isize::MAX as usize, OnError::ReturnErr);
        assert!(result.is_err());
        assert!(instance.pointer.is_null());
    }

    #[test]
    fn test_index_store_read_tags() {
        let mut instance = MapIndex::new_unallocated();
        instance.allocate_uninit(10, OnError::NoReturn).unwrap();

        unsafe {
            instance.set_tags_empty(10);

            for i in 0..10 {
                assert!(instance.read_tag(i).is_empty());
            }

            for i in 0..10 {
                instance.store_tag(i, Tag::Occupied)
            }

            for i in 0..10 {
                assert!(instance.read_tag(i).is_occupied());
            }

            instance.deallocate(10)
        }
    }

    #[test]
    fn test_index_store_read_entry_index() {
        let mut instance = MapIndex::new_unallocated();
        instance.allocate_uninit(10, OnError::NoReturn).unwrap();

        unsafe {
            instance.set_tags_empty(10);

            for i in 0..10 {
                instance.store_entry_index(i, 11)
            }

            for i in 0..10 {
                assert_eq!(instance.read_entry_index(i), 11);
            }

            instance.deallocate(10)
        }
    }

    #[test]
    fn test_index_initialize_from() {
        let mut source = MapIndex::new_unallocated();
        source.allocate_uninit(10, OnError::NoReturn).unwrap();

        unsafe {
            source.set_tags_empty(10);

            for i in 0..10 {
                source.store_tag(i, Tag::Occupied)
            }

            for i in 0..10 {
                source.store_entry_index(i, 11)
            }
        }

        let mut target = MapIndex::new_unallocated();
        let _ = target.allocate_uninit(10, OnError::NoReturn).unwrap();

        unsafe {
            target.copy_from(&source, 10);

            for i in 0..10 {
                assert!(target.read_tag(i).is_occupied());
            }

            for i in 0..10 {
                assert_eq!(target.read_entry_index(i), 11);
            }
        }

        unsafe {
            source.deallocate(10);
            target.deallocate(10)
        }
    }

    #[test]
    fn test_index_reset_control_tags() {
        let mut instance = MapIndex::new_unallocated();
        instance.allocate_uninit(10, OnError::NoReturn).unwrap();

        unsafe {
            instance.set_tags_empty(10);

            for i in 0..10 {
                instance.store_tag(i, Tag::Occupied)
            }

            instance.set_tags_empty(10);

            for i in 0..10 {
                assert!(instance.read_tag(i).is_empty());
            }

            instance.deallocate(10);
        }
    }

    #[test]
    fn test_index_scope_guard() {
        let mut instance = MapIndex::new_unallocated();
        instance.allocate_uninit(10, OnError::NoReturn).unwrap();
        assert!(!instance.pointer.is_null());

        unsafe {
            let _ = instance.guard(10);
            // Out of scope, dropped.
        }

        // Deallocated.
        assert!(instance.pointer.is_null());
    }

    #[test]
    fn test_index_scope_guard_deactivate() {
        let mut instance = MapIndex::new_unallocated();
        instance.allocate_uninit(10, OnError::NoReturn).unwrap();
        assert!(!instance.pointer.is_null());

        unsafe {
            let guard = instance.guard(10);
            guard.deactivate();
            // Out of scope.
        }

        // Still allocated.
        assert!(!instance.pointer.is_null());
        unsafe {
            instance.deallocate(10);
        }
    }
}
