## OmniMap

A key-value map data structure that maintains the insertion order of items.

## Features
- Relatively simple and compact implementation.
- Maintains the order in which items are inserted.
- Order preservation of items during all operations including: insertion, updating **and** removing.
- Optimized for fast access.
- Set of very useful methods and functions inspired by both hash maps and vectors.

## Future Improvements
- Better capacity and resizing strategy.
- Some lazy operations.

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

### Create new OmniMap with default capacity
```rust
use omni_map::OmniMap;

let map: OmniMap<String, i32> = OmniMap::default();

assert!(map.is_empty());

assert_eq!(map.len(), 0);

assert_eq!(map.capacity(), 16);
```

### Insert items into the map with order preservation
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
    // Compiler will not always infer the type of the key, so you may need to specify it
    map.iter().collect::<Vec<(&String, &i32)>>(),
    vec![
        (&"key1".to_string(),&1),
        (&"key2".to_string(),&2),
        (&"key3".to_string(),&3)
    ]
);
```

### Access items in the map
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

### Access first and last items in the map
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

### Update items in the map
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

### Access first and last items in the map
```rust
use omni_map::OmniMap;

let mut map = OmniMap::new();

map.upsert("key1".to_string(), 1);
map.upsert("key2".to_string(), 2);
map.upsert("key3".to_string(), 3);

assert_eq!(map.first(), Some((&"key1".to_string(), &1)));

assert_eq!(map.last(), Some((&"key3".to_string(), &3));
```

### Remove items and preserve order
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

// Order of the keys should be preserved, but index has been updated
// Access remaining item by key
assert_eq!(map.get(&"key3".to_string()), Some(&3));
```

### Pop the first item in the map
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

### Pop the last item in the map
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

### Reserve extra capacity
```rust
let mut map = OmniMap::new();

assert_eq!(map.capacity(), 0);

map.upsert("key1".to_string(), 1);

assert_eq!(map.capacity(), 1);

// Reserve capacity in advance
map.reserve_capacity(1000);

assert_eq!(map.capacity(), 1001);
```
