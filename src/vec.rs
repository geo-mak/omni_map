use std::alloc::{self, Layout};
use std::marker::PhantomData;
use std::ops::{Index, IndexMut};
use std::ptr::{self, NonNull};

/// Raw vector to enable better control over memory allocation and reallocation and capacity growth.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AllocVec<T> {
    ptr: NonNull<T>,
    cap: usize,
    len: usize,
    _marker: PhantomData<T>,
}

impl<T> AllocVec<T> {
    /// Creates a new, empty `AllocVec`.
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
    #[inline]
    pub(crate) fn with_capacity(cap: usize) -> Self {
        if cap == 0 {
            return Self::new();
        }
        let layout = Layout::array::<T>(cap).unwrap();
        let ptr = unsafe { alloc::alloc(layout) as *mut T };
        let ptr = NonNull::new(ptr).unwrap_or_else(|| alloc::handle_alloc_error(layout));
        AllocVec {
            ptr,
            cap,
            len: 0,
            _marker: PhantomData,
        }
    }

    /// Reserves capacity for at least `additional` more elements.
    /// The resulted capacity will be `self.capacity() + additional`.
    ///
    /// # Arguments
    ///
    /// * `additional` - The number of additional elements to reserve space for.
    ///
    /// # Time Complexity
    /// - *O*(n) where n is the new capacity.
    ///
    pub(crate) fn reserve(&mut self, additional: usize) {
        let new_cap = self.cap.checked_add(additional).expect("capacity overflow");
        if new_cap > self.cap {
            self.reallocate(new_cap);
        }
    }

    /// Reallocates the vector to a new capacity.
    fn reallocate(&mut self, new_cap: usize) {
        let new_layout = Layout::array::<T>(new_cap).expect("layout error");
        let new_ptr = if self.cap == 0 {
            unsafe { alloc::alloc(new_layout) as *mut T }
        } else {
            let old_layout = Layout::array::<T>(self.cap).expect("layout error");
            unsafe {
                alloc::realloc(self.ptr.as_ptr() as *mut u8, old_layout, new_layout.size())
                    as *mut T
            }
        };
        self.ptr = NonNull::new(new_ptr).expect("allocation error");
        self.cap = new_cap;
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
            // Safety first, write first
            ptr::write(self.ptr.as_ptr().add(self.len), value);
        }
        self.len += 1;
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
    #[inline]
    pub(crate) fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        if index < self.len {
            unsafe { Some(&mut *self.ptr.as_ptr().add(index)) }
        } else {
            None
        }
    }

    /// Returns a reference to the first element, or `None` if the `AllocVec` is empty.
    ///
    /// # Time Complexity
    /// - *O*(1).
    ///
    #[inline]
    pub(crate) fn first(&self) -> Option<&T> {
        if self.len == 0 {
            None
        } else {
            unsafe { Some(&*self.ptr.as_ptr()) }
        }
    }

    /// Returns a reference to the last element, or `None` if the `AllocVec` is empty.
    ///
    /// # Time Complexity
    /// - *O*(1).
    ///
    #[inline]
    pub(crate) fn last(&self) -> Option<&T> {
        if self.len == 0 {
            None
        } else {
            unsafe { Some(&*self.ptr.as_ptr().add(self.len - 1)) }
        }
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
    ///
    /// O(n) where n is the length of the `AllocVec`
    pub(crate) fn remove(&mut self, index: usize) -> T {
        assert!(index < self.len);
        // Safety first, update len first
        self.len -= 1;
        unsafe {
            let ptr = self.ptr.as_ptr().add(index);
            // Read the value and unsafely make copy of the value on
            // the stack and in the vector at the same time.
            let value = ptr::read(ptr);
            // Shift everything to fill in.
            ptr::copy(ptr.add(1), ptr, self.len - index);
            value
        }
    }

    /// Removes the last element and returns it, or `None` if the `AllocVec` is empty.
    ///
    /// # Time Complexity
    /// - *O*(1).
    ///
    #[inline]
    pub(crate) fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            None
        } else {
            // Safety first, update len first
            self.len -= 1;
            unsafe { Some(ptr::read(self.ptr.as_ptr().add(self.len))) }
        }
    }

    /// Removes the first element and returns it, or `None` if the `AllocVec` is empty.
    ///
    /// # Time Complexity
    /// - *O*(n) where n is the length of the `AllocVec`.
    ///
    pub(crate) fn pop_front(&mut self) -> Option<T> {
        if self.len == 0 {
            None
        } else {
            // Read the value and unsafely make copy of the value on
            // the stack and in the vector at the same time.
            let value = unsafe { ptr::read(self.ptr.as_ptr()) };
            // Safety first, update len first
            self.len -= 1;
            unsafe {
                // Shift everything to fill in.
                ptr::copy(self.ptr.as_ptr().add(1), self.ptr.as_ptr(), self.len);
            }
            Some(value)
        }
    }

    /// Clears the `AllocVec`, removing all elements.
    ///
    /// # Time Complexity
    /// - *O*(n) where n is the length of the `AllocVec`.
    ///
    #[inline]
    pub(crate) fn clear(&mut self) {
        if self.len != 0 {
            // Safety first, update len first
            self.len = 0;
            // Drop in place
            unsafe {
                ptr::drop_in_place(std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len));
            }
        }
    }

    /// Resizes the `AllocVec` to the specified length, using the provided function to generate new elements.
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
            if new_len > self.cap {
                self.reserve(new_len - self.len);
            }
            for _ in self.len..new_len {
                self.push(f());
            }
        } else {
            for _ in new_len..self.len {
                self.pop();
            }
        }
    }

    /// Returns an iterator over the elements of the `AllocVec`.
    ///
    /// # Time Complexity
    /// - *O*(1).
    ///
    pub(crate) fn iter(&self) -> std::slice::Iter<'_, T> {
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.len).iter() }
    }

    /// Returns a mutable iterator over the elements of the `AllocVec`.
    ///
    /// # Time Complexity
    ///
    /// O(1)
    pub(crate) fn iter_mut(&mut self) -> std::slice::IterMut<'_, T> {
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len).iter_mut() }
    }
}

impl<T> Drop for AllocVec<T> {
    /// Drops the `AllocVec`, deallocating its memory.
    fn drop(&mut self) {
        // Check allocated capacity
        if self.cap != 0 {
            // Create layout
            let layout = Layout::array::<T>(self.cap).unwrap();
            unsafe {
                for i in 0..self.len {
                    ptr::drop_in_place(self.ptr.as_ptr().add(i));
                }
                // Deallocate the memory space as defined by the layout
                alloc::dealloc(self.ptr.as_ptr() as *mut u8, layout);
            }
        }
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

impl<T: Default> AllocVec<T> {
    /// Creates a new `AllocVec` with the specified capacity and populates it with the default value of `T`.
    ///
    /// # Arguments
    ///
    /// * `cap` - The capacity of the new `AllocVec`.
    pub(crate) fn with_capacity_and_populate(cap: usize) -> Self {
        if cap == 0 {
            return Self::new();
        }
        let layout = Layout::array::<T>(cap).unwrap();
        let ptr = unsafe { alloc::alloc(layout) as *mut T };
        let ptr = NonNull::new(ptr).unwrap_or_else(|| alloc::handle_alloc_error(layout));

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
    raw_vec: AllocVec<T>,
    index: usize,
}

impl<T> Iterator for AllocVecIntoIter<T> {
    type Item = T;

    /// Returns the next element in the iterator.
    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.raw_vec.len {
            let item = unsafe { ptr::read(self.raw_vec.ptr.as_ptr().add(self.index)) };
            self.index += 1;
            Some(item)
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
            raw_vec: self,
            index: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_raw_vec_new() {
        let raw_vec: AllocVec<i32> = AllocVec::new();
        assert_eq!(raw_vec.capacity(), 0);
        assert_eq!(raw_vec.len(), 0);
        assert!(raw_vec.is_empty());
    }

    #[test]
    fn test_raw_vec_with_capacity() {
        let raw_vec: AllocVec<i32> = AllocVec::with_capacity(10);
        assert_eq!(raw_vec.capacity(), 10);
        assert_eq!(raw_vec.len(), 0);
        assert!(raw_vec.is_empty());
    }

    #[test]
    fn test_raw_vec_reserve() {
        let mut raw_vec: AllocVec<i32> = AllocVec::with_capacity(10);
        raw_vec.reserve(5);
        assert_eq!(raw_vec.capacity(), 15);
    }

    #[test]
    fn test_raw_vec_capacity() {
        let raw_vec: AllocVec<i32> = AllocVec::with_capacity(10);
        assert_eq!(raw_vec.capacity(), 10);
    }

    #[test]
    fn test_raw_vec_len() {
        let raw_vec: AllocVec<i32> = AllocVec::with_capacity(10);
        assert_eq!(raw_vec.len(), 0);
    }

    #[test]
    fn test_raw_vec_push() {
        let mut raw_vec: AllocVec<i32> = AllocVec::with_capacity(10);
        raw_vec.push(42);
        assert_eq!(raw_vec.len(), 1);
        assert_eq!(raw_vec[0], 42);
        assert!(!raw_vec.is_empty());
    }

    #[test]
    fn test_raw_vec_get() {
        let mut raw_vec: AllocVec<i32> = AllocVec::with_capacity(10);
        raw_vec.push(1);
        raw_vec.push(2);
        raw_vec.push(3);
        assert_eq!(raw_vec.get(0), Some(&1));
        assert_eq!(raw_vec.get(1), Some(&2));
        assert_eq!(raw_vec.get(2), Some(&3));
        assert_eq!(raw_vec.get(3), None);
    }

    #[test]
    fn test_raw_vec_get_mut() {
        let mut raw_vec: AllocVec<i32> = AllocVec::with_capacity(10);
        raw_vec.push(1);
        raw_vec.push(2);
        raw_vec.push(3);
        if let Some(value) = raw_vec.get_mut(1) {
            *value = 42;
        }
        assert_eq!(raw_vec.get(1), Some(&42));
    }

    #[test]
    fn test_raw_vec_index() {
        let mut raw_vec: AllocVec<i32> = AllocVec::with_capacity(10);
        raw_vec.push(42);
        assert_eq!(raw_vec[0], 42);
    }

    #[test]
    #[should_panic(expected = "Index out of bounds")]
    fn test_raw_vec_index_out_of_bounds() {
        let mut raw_vec: AllocVec<i32> = AllocVec::with_capacity(10);
        raw_vec.push(42);

        let _ = raw_vec[1];
    }

    #[test]
    fn test_raw_vec_index_mut() {
        let mut raw_vec: AllocVec<i32> = AllocVec::with_capacity(10);
        raw_vec.push(42);
        raw_vec[0] = 43;
        assert_eq!(raw_vec[0], 43);
    }

    #[test]
    fn test_raw_vec_first() {
        let mut raw_vec: AllocVec<i32> = AllocVec::with_capacity(10);
        assert_eq!(raw_vec.first(), None);
        raw_vec.push(1);
        raw_vec.push(2);
        assert_eq!(raw_vec.first(), Some(&1));
    }

    #[test]
    fn test_raw_vec_last() {
        let mut raw_vec: AllocVec<i32> = AllocVec::with_capacity(10);
        assert_eq!(raw_vec.last(), None);
        raw_vec.push(1);
        raw_vec.push(2);
        assert_eq!(raw_vec.last(), Some(&2));
    }

    #[test]
    fn test_raw_vec_pop_front() {
        let mut raw_vec: AllocVec<i32> = AllocVec::with_capacity(10);
        raw_vec.push(1);
        raw_vec.push(2);
        raw_vec.push(3);
        assert_eq!(raw_vec.pop_front(), Some(1));
        assert_eq!(raw_vec.len(), 2);
        assert_eq!(raw_vec[0], 2);
        assert_eq!(raw_vec[1], 3);
    }

    #[test]
    fn test_raw_vec_pop() {
        let mut raw_vec: AllocVec<i32> = AllocVec::with_capacity(10);
        raw_vec.push(42);
        assert_eq!(raw_vec.pop(), Some(42));
        assert_eq!(raw_vec.len(), 0);
        assert!(raw_vec.is_empty());
    }

    #[test]
    fn test_raw_vec_remove() {
        let mut raw_vec: AllocVec<i32> = AllocVec::with_capacity(10);
        raw_vec.push(42);
        raw_vec.push(43);
        assert_eq!(raw_vec.remove(0), 42);
        assert_eq!(raw_vec.len(), 1);
        assert_eq!(raw_vec[0], 43);
    }

    #[test]
    fn test_raw_vec_resize_with() {
        let mut raw_vec: AllocVec<i32> = AllocVec::with_capacity(10);
        raw_vec.resize_with(5, || 42);
        assert_eq!(raw_vec.len(), 5);
        for i in 0..5 {
            assert_eq!(raw_vec[i], 42);
        }
        raw_vec.resize_with(2, || 0);
        assert_eq!(raw_vec.len(), 2);
        assert_eq!(raw_vec[0], 42);
        assert_eq!(raw_vec[1], 42);
    }

    #[test]
    fn test_raw_vec_iter() {
        let mut raw_vec: AllocVec<i32> = AllocVec::with_capacity(10);
        raw_vec.push(1);
        raw_vec.push(2);
        raw_vec.push(3);
        let mut iter = raw_vec.iter();
        assert_eq!(iter.next(), Some(&1));
        assert_eq!(iter.next(), Some(&2));
        assert_eq!(iter.next(), Some(&3));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn test_raw_vec_iter_mut() {
        let mut raw_vec: AllocVec<i32> = AllocVec::with_capacity(10);
        raw_vec.push(1);
        raw_vec.push(2);
        raw_vec.push(3);
        for value in raw_vec.iter_mut() {
            *value *= 2;
        }
        let mut iter = raw_vec.iter();
        assert_eq!(iter.next(), Some(&2));
        assert_eq!(iter.next(), Some(&4));
        assert_eq!(iter.next(), Some(&6));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn test_raw_vec_clear() {
        let mut raw_vec: AllocVec<i32> = AllocVec::with_capacity(10);
        raw_vec.push(1);
        raw_vec.push(2);
        raw_vec.push(3);
        raw_vec.clear();
        assert_eq!(raw_vec.len(), 0);
        assert!(raw_vec.is_empty());
    }

    #[test]
    fn test_raw_vec_for_loop() {
        let mut raw_vec: AllocVec<i32> = AllocVec::with_capacity(10);
        raw_vec.push(1);
        raw_vec.push(2);
        raw_vec.push(3);

        let mut sum = 0;
        for value in &raw_vec {
            sum += *value;
        }
        assert_eq!(sum, 6);

        for value in &mut raw_vec {
            *value *= 2;
        }

        let mut iter = raw_vec.iter();
        assert_eq!(iter.next(), Some(&2));
        assert_eq!(iter.next(), Some(&4));
        assert_eq!(iter.next(), Some(&6));
        assert_eq!(iter.next(), None);

        let raw_vec = raw_vec;
        let mut iter = raw_vec.into_iter();
        assert_eq!(iter.next(), Some(2));
        assert_eq!(iter.next(), Some(4));
        assert_eq!(iter.next(), Some(6));
        assert_eq!(iter.next(), None);
    }
}
