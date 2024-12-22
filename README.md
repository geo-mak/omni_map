## OmniMap

A key-value data structure that combines the best of both hash maps and vectors.

## Features
- Relatively simple and compact implementation.
- Maintains the order in which items are inserted.
- Order preservation of items during all operations including: insertion, updating and **removing**.
- Optimized for fast access.
- Set of very useful methods and functions inspired by both hash maps and vectors.

## **Notes**:
- Original implementation used separate chaining for collision resolution, but switched to open addressing with linear probing to minimize memory usage. Other implementations with open addressing use other probing formulas to prevent clustering, but testing showed that linear probing with the current load factor is more efficient in most cases.

- Removing items from the map is a relatively expensive operation compared to regular hash maps, 
because it requires shifting all items after the removed item in a **dense vector** and updating their indices to maintain order. Other implementations use sparse vectors with marking instead of actual removing, 
but they have slower iteration with compaction overhead and without allowing access to value **by index**.
Nevertheless, the current implementation assumes that removing is not a very frequent operation with very large number of items.

- The map is currently not thread-safe, so a mutex or other synchronization mechanism are needed for usage in multithreaded environments.

## Examples

### Create a new OmniMap without initial capacity
```rust
use omni_map::OmniMap;

// The map will allocate with first insertion and will grow as needed.
let map: OmniMap<i32, &str> = OmniMap::new();

assert!(map.is_empty());

assert_eq!(map.len(), 0);

assert_eq!(map.capacity(), 0);
```

### Create a new OmniMap with a capacity
```rust
use omni_map::OmniMap;

// Creating a map with a capacity is much more efficient.
// The map will reallocate when the load factor is exceeded.
let map: OmniMap<i32, &str> = OmniMap::with_capacity(100);

assert!(map.is_empty());

assert_eq!(map.len(), 0);

assert_eq!(map.capacity(), 100);
```

### Creating new OmniMap with default capacity
```rust
use omni_map::OmniMap;

let map: OmniMap<String, i32> = OmniMap::default();

assert!(map.is_empty());

assert_eq!(map.len(), 0);

assert_eq!(map.capacity(), 16);
```

### Inserting new items into the map with order preservation
```rust
use omni_map::OmniMap;

let mut map = OmniMap::new();

map.insert(1, "a");
map.insert(2, "b");
map.insert(3, "c");

assert!(!map.is_empty());
assert_eq!(map.len(), 3);

// Order of the items
assert_eq!(
    map.iter().collect::<Vec<(&i32, &&str)>>(),
    vec![
        (&1, &"a"),
        (&2, &"b"),
        (&3, &"c")
    ]
);
```

### Updating value of an existing key
```rust
use omni_map::OmniMap;

let mut map = OmniMap::new();

map.insert(1, "a");

// When updating an existing key, the old value is returned.
let old_value = map.insert(1, "b");

// Length must be 1.
assert_eq!(map.len(), 1);

// Old value must be "a".
assert_eq!(old_value, Some("a"));

// New value must be "b".
assert_eq!(map.get(&1), Some(&"b"));
```

### **Immutable** access to value by key
```rust
use omni_map::OmniMap;

let mut map = OmniMap::new();

map.insert(1, "a");
map.insert(2, "b");
map.insert(3, "c");

assert_eq!(map.get(&1), Some(&"a"));
assert_eq!(map.get(&2), Some(&"b"));
assert_eq!(map.get(&3), Some(&"c"));
```

### **Mutable** access to value by key
```rust
use omni_map::OmniMap;

let mut map = OmniMap::new();

map.insert(1, "a");

if let Some(value) = map.get_mut(&1) {
// Mutate the value
*value = "b";
}

// Value of `1` has been mutated
assert_eq!(map.get(&1), Some(&"b"));
```

### **Immutable** access to value by index
```rust
use omni_map::OmniMap;

let mut map = OmniMap::new();

map.insert(1, "a");
map.insert(2, "b");
map.insert(3, "c");

assert_eq!(map[0], "a");
assert_eq!(map[1], "b");
assert_eq!(map[2], "c");

// Remove the first item
map.pop_front();

// The first item now must be the second item
// The second item now must be the third item
assert_eq!(map[0], "b");
assert_eq!(map[1], "c");
```

### **Mutable** access to value by index
```rust
use omni_map::OmniMap;

let mut map = OmniMap::new();

map.insert(1, "a");
map.insert(2, "b");
map.insert(3, "c");

// Mutate the values by index
map[0] = "x";
map[1] = "y";
map[2] = "z";

assert_eq!(map[0], "x");
assert_eq!(map[1], "y");
assert_eq!(map[2], "z");
```

### **Immutable** access to the first and last items in the map
```rust
use omni_map::OmniMap;

let mut map = OmniMap::new();

map.insert(1, "a");
map.insert(2, "b");
map.insert(3, "c");

// First key is 1 with value "a"
assert_eq!(map.first(), Some((&1, &"a")));

// Last key is 3 with value "c"
assert_eq!(map.last(), Some((&3, &"c")));
```

### Removing items and preserve order
```rust
use omni_map::OmniMap;

let mut map = OmniMap::new();

// Insert 4 items
map.insert(1, "a");
map.insert(2, "b");
map.insert(3, "c");
map.insert(4, "d");

assert_eq!(map.len(), 4);

// Remove the second item (2)
// Must return true
assert!(map.remove(&2));

// Length is now 3
assert_eq!(map.len(), 3);

// Check the order of the remaining items
assert_eq!(
    map.iter().collect::<Vec<(&i32, &&str)>>(),
    vec![
        (&1, &"a"),
        (&3, &"c"),
        (&4, &"d")
    ]
);
```

### Removing the first item in the map
```rust
use omni_map::OmniMap;

let mut map = OmniMap::new();

map.insert(1, "a");
map.insert(2, "b");
map.insert(3, "c");

assert_eq!(map.len(), 3);

// Pop the first item
let removed_item = map.pop_front();
assert_eq!(removed_item, Some((1, "a")));

// length is now 2
assert_eq!(map.len(), 2);

// First key is removed
assert_eq!(map.get(&1), None);

// First key now must be the second key
assert_eq!(map.first(), Some((&2, &"b")));
```

### Removing the last item in the map
```rust
use omni_map::OmniMap;

let mut map = OmniMap::new();

map.insert(1, "a");
map.insert(2, "b");
map.insert(3, "c");

assert_eq!(map.len(), 3);

// Pop the last item
let removed_item = map.pop();

assert_eq!(removed_item, Some((3, "c")));

// length is now 2
assert_eq!(map.len(), 2);

// Last key is removed
assert_eq!(map.get(&3), None);

// Last key now must be the second key
assert_eq!(map.last(), Some((&2, &"b")));
```

### Reserving extra capacity
```rust
use omni_map::OmniMap;

let mut map = OmniMap::new();

assert_eq!(map.capacity(), 0);

map.insert(1, "a");

assert_eq!(map.capacity(), 1);

// Reserve capacity in advance
map.reserve_capacity(10);

assert_eq!(map.capacity(), 11);
```

### Shrinking the capacity to fit the number of items
```rust
let mut map = OmniMap::new();

assert_eq!(map.capacity(), 0);

for i in 0..10 {
map.insert(i, i);
}

assert_eq!(map.capacity(), 16);

// Shrink the capacity to fit
map.shrink_to_fit();

assert_eq!(, 10);

// Capacity is now equal to the number of items
assert_eq!(map.capacity(), 10);
```

### Shrinking the capacity to a specific capacity
```rust
let mut map = OmniMap::new();

assert_eq!(map.capacity(), 0);

for i in 0..10 {
map.insert(i, i);
}

assert_eq!(map.capacity(), 16);

// Shrink the capacity to 12
map.shrink_to(12);

assert_eq!(map.len(), 10);

// Capacity has been shrunk to 12
assert_eq!(map.capacity(), 12);
```
