## OmniMap

A hybrid data structure that combines the best of both hash maps and vectors.

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
but they have higher memory footprint and slower iteration with compacting overhead and without allowing access to value **by index**.
Nevertheless, the current implementation assumes that removing is not a very frequent operation with very large number of items.

- The map is currently not thread-safe, so a mutex or other synchronization mechanism are needed for usage in multithreaded environments.

## Examples

### Create a new OmniMap without initial capacity
```rust
use omni_map::OmniMap;

// The map will allocate with first insertion and will grow as needed.
let map: OmniMap<String, i32> = OmniMap::new();

assert!(map.is_empty());

assert_eq!(map.len(), 0);

assert_eq!(map.capacity(), 0);
```

### Create a new OmniMap with a capacity
```rust
use omni_map::OmniMap;

// Creating a map with a capacity is much more efficient.
// The map will reallocate when the load factor is exceeded.
let map: OmniMap<String, i32> = OmniMap::with_capacity(1000);

assert!(map.is_empty());

assert_eq!(map.len(), 0);

assert_eq!(map.capacity(), 1000);
```

### Creating new OmniMap with default capacity
```rust
use omni_map::OmniMap;

let map: OmniMap<String, i32> = OmniMap::default();

assert!(map.is_empty());

assert_eq!(map.len(), 0);

assert_eq!(map.capacity(), 16);
```

### Inserting items into the map with order preservation
```rust
use omni_map::OmniMap;

let mut map = OmniMap::new();

map.upsert("key1", 1);
map.upsert("key2", 2);
map.upsert("key3", 3);

assert!(!map.is_empty());
assert_eq!(map.len(), 3);

// Order of the items
assert_eq!(
    map.iter().collect::<Vec<(&String, &i32)>>(),
    vec![
        (&"key1".to_string(),&1),
        (&"key2".to_string(),&2),
        (&"key3".to_string(),&3)
    ]
);
```

### **Immutable** access to value by key
```rust
use omni_map::OmniMap;

let mut map = OmniMap::new();

map.upsert("key1", 1);
map.upsert("key2", 2);
map.upsert("key3", 3);

assert_eq!(map.get("key1"), Some(&1));
assert_eq!(map.get("key2"), Some(&2));
assert_eq!(map.get("key3"), Some(&3));
```

### **Mutable** access to value by key
```rust
use omni_map::OmniMap;

let mut map = OmniMap::new();

map.upsert("key1".to_string(), 1);

if let Some(value) = map.get_mut(&"key1".to_string()) {
        // Mutate the value
            *value = 10;
        }

// Value of `key1` has been mutated
assert_eq!(map.get(&"key1".to_string()), Some(&10));
```

### **Immutable** access to value by index
```rust
use omni_map::OmniMap;

let mut map = OmniMap::new();

map.upsert("key1".to_string(), 1);
map.upsert("key2".to_string(), 2);
map.upsert("key3".to_string(), 3);

assert_eq!(map[0], 1);
assert_eq!(map[1], 2);
assert_eq!(map[2], 3);

// Remove the first item
map.pop_front();

// The first item now must be the second item
// The second item now must be the third item
assert_eq!(map[0], 2);
assert_eq!(map[1], 3);
```

### **Mutable** access to value by index
```rust
use omni_map::OmniMap;

let mut map = OmniMap::new();

map.upsert("key1".to_string(), 1);
map.upsert("key2".to_string(), 2);
map.upsert("key3".to_string(), 3);

// Mutate the values by index
map[0] = 10;
map[1] = 20;
map[2] = 30;

assert_eq!(map[0], 10);
assert_eq!(map[1], 20);
assert_eq!(map[2], 30);
```

### **Immutable** access to the first and last items in the map
```rust
use omni_map::OmniMap;

let mut map = OmniMap::new();

map.upsert("key1".to_string(), 1);
map.upsert("key2".to_string(), 2);
map.upsert("key3".to_string(), 3);

// First key is "key1" with value 1
assert_eq!(map.first(), Some((&"key1".to_string(), &1)));

// Last key is "key3" with value 3
assert_eq!(map.last(), Some((&"key3".to_string(), &3)));
```

### Updating value of an existing key
```rust
use omni_map::OmniMap;

let mut map = OmniMap::new();

map.upsert("key1".to_string(), 1);

// Update the value of the same key
map.upsert("key1".to_string(), 2);

// Length must be 1
assert_eq!(map.len(), 1);

assert_eq!(map.get(&"key1".to_string()), Some(&2));
```

### Removing items and preserve order
```rust
use omni_map::OmniMap;

let mut map = OmniMap::new();

// Insert 4 items
map.upsert("key1".to_string(), 1);
map.upsert("key2".to_string(), 2);
map.upsert("key3".to_string(), 3);
map.upsert("key4".to_string(), 4);

assert_eq!(map.len(), 4);

// Remove the second item ("key2")
// Must return true
assert!(map.remove(&"key2".to_string()));

// Length is now 3
assert_eq!(map.len(), 3);

// Check the order of the remaining items
assert_eq!(
    map.iter().collect::<Vec<(&String, &i32)>>(),
    vec![
        (&"key1".to_string(),&1),
        (&"key3".to_string(),&3),
        (&"key4".to_string(),&4)
    ]
);
```

### Removing the first item in the map
```rust
use omni_map::OmniMap;

let mut map = OmniMap::new();

map.upsert("key1".to_string(), 1);
map.upsert("key2".to_string(), 2);
map.upsert("key3".to_string(), 3);

assert_eq!(map.len(), 3);

// Pop the first item
let removed_item = map.pop_front();
assert_eq!(removed_item, Some(("key1".to_string(), 1)));

// length is now 2
assert_eq!(map.len(), 2);

// First key is removed
assert_eq!(map.get(&"key1".to_string()), None);

// First key now must be the second key
assert_eq!(map.first(), Some((&"key2".to_string(), &2)));
```

### Removing the last item in the map
```rust
use omni_map::OmniMap;

let mut map = OmniMap::new();

map.upsert("key1".to_string(), 1);
map.upsert("key2".to_string(), 2);
map.upsert("key3".to_string(), 3);

assert_eq!(map.len(), 3);

// Pop the last item
let removed_item = map.pop();

assert_eq!(removed_item, Some(("key3".to_string(), 3)));

// length is now 2
assert_eq!(map.len(), 2);

// Last key is removed
assert_eq!(map.get(&"key3".to_string()), None);

// Last key now must be the second key
assert_eq!(map.last(), Some((&"key2".to_string(), &2)));
```

### Reserving extra capacity
```rust
let mut map = OmniMap::new();

assert_eq!(map.capacity(), 0);

map.upsert("key1".to_string(), 1);

assert_eq!(map.capacity(), 1);

// Reserve capacity in advance
map.reserve_capacity(1000);

assert_eq!(map.capacity(), 1001);
```

### Shrinking the capacity to fit the number of items
```rust
let mut map = OmniMap::new();

assert_eq!(map.capacity(), 0);

for i in 0..10 {
map.upsert(i, i);
}

assert_eq!(map.capacity(), 16);

// Shrink the capacity to fit
map.shrink_to_fit();

assert_eq!(, 10);

assert_eq!(map.capacity(), 10); // Capacity is now equal to the number of items
```

### Shrinking the capacity to a specific capacity
```rust
let mut map = OmniMap::new();

assert_eq!(map.capacity(), 0);

for i in 0..10 {
map.upsert(i, i);
}

assert_eq!(map.capacity(), 16);

// Shrink the capacity to 12
map.shrink_to(12);

assert_eq!(map.len(), 10);

assert_eq!(map.capacity(), 12); // Capacity has been shrunk to 12
```