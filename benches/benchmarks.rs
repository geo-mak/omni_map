use criterion::{black_box, criterion_group, criterion_main, Criterion};
use omni_map::OmniMap;
use std::collections::HashMap;

// black_box is used to prevent the compiler from optimizing the code away

fn bench_upsert(c: &mut Criterion) {
    let mut map = OmniMap::new();
    c.bench_function("upsert 10_000 items", |b| {
        b.iter(|| {
            for i in 0..10_000 {
                black_box(map.upsert(i.to_string(), i));
            }
        })
    });
}

fn bench_upsert_hashmap(c: &mut Criterion) {
    let mut map = HashMap::new();
    c.bench_function("upsert 10_000 items (HashMap)", |b| {
        b.iter(|| {
            for i in 0..10_000 {
                black_box(map.insert(i.to_string(), i));
            }
        })
    });
}

fn bench_get(c: &mut Criterion) {
    let mut map = OmniMap::new();
    for i in 0..10_000 {
        map.upsert(i.to_string(), i);
    }
    c.bench_function("get item", |b| {
        b.iter(|| {
            black_box(map.get(&"5000".to_string()));
        })
    });
}

fn bench_get_hashmap(c: &mut Criterion) {
    let mut map = HashMap::new();
    for i in 0..10_000 {
        map.insert(i.to_string(), i);
    }
    c.bench_function("get item (HashMap)", |b| {
        b.iter(|| {
            black_box(map.get(&"5000".to_string()));
        })
    });
}

fn bench_first(c: &mut Criterion) {
    let mut map = OmniMap::new();
    for i in 0..10_000 {
        map.upsert(i.to_string(), i);
    }
    c.bench_function("get first item", |b| {
        b.iter(|| {
            black_box(map.first());
        })
    });
}

fn bench_last(c: &mut Criterion) {
    let mut map = OmniMap::new();
    for i in 0..10_000 {
        map.upsert(i.to_string(), i);
    }
    c.bench_function("get last item", |b| {
        b.iter(|| {
            black_box(map.last());
        })
    });
}

fn bench_remove(c: &mut Criterion) {
    c.bench_function("remove item", |b| {
        let mut map = OmniMap::new();
        for i in 0..10_000 {
            map.upsert(i.to_string(), i);
        }
        b.iter(|| {
            black_box(map.remove(&"1".to_string()));
        })
    });
}

fn bench_remove_hashmap(c: &mut Criterion) {
    c.bench_function("remove item (HashMap)", |b| {
        let mut map = HashMap::new();
        for i in 0..10_000 {
            map.insert(i.to_string(), i);
        }
        b.iter(|| {
            black_box(map.remove(&"1".to_string()));
        })
    });
}

fn bench_pop_first(c: &mut Criterion) {
    c.bench_function("pop first item", |b| {
        let mut map = OmniMap::new();
        for i in 0..10_000 {
            map.upsert(i.to_string(), i);
        }
        b.iter(|| {
            black_box(map.pop_front());
        })
    });
}

fn bench_pop_last(c: &mut Criterion) {
    c.bench_function("pop last item", |b| {
        let mut map = OmniMap::new();
        for i in 0..10_000 {
            map.upsert(i.to_string(), i);
        }
        b.iter(|| {
            black_box(map.pop());
        })
    });
}

fn bench_shrink_to(c: &mut Criterion) {
    // Capacity is 16384, which is 2048 more than HashMap with 10_000 items
    c.bench_function("shrink to 11_000", |b| {
        let mut map = OmniMap::new();
        for i in 0..10_000 {
            map.upsert(i.to_string(), i);
        }
        b.iter(|| {
            black_box(map.shrink_to(11_000));
        })
    });
}

fn bench_shrink_to_fit(c: &mut Criterion) {
    // Capacity is 16384, which is 2048 more than HashMap with 10_000 items
    c.bench_function("shrink to fit", |b| {
        let mut map = OmniMap::new();
        for i in 0..10_000 {
            map.upsert(i.to_string(), i);
        }
        b.iter(|| {
            black_box(map.shrink_to_fit());
        })
    });
}

fn bench_shrink_to_hashmap(c: &mut Criterion) {
    // Capacity is 14336, which 2048 less than OmniMap with 10_000 items
    c.bench_function("shrink to 11_000 (HashMap)", |b| {
        let mut map = HashMap::new();
        for i in 0..10_000 {
            map.insert(i.to_string(), i);
        }
        b.iter(|| {
            black_box(map.shrink_to(11_000));
        })
    });
}

fn bench_shrink_to_fit_hashmap(c: &mut Criterion) {
    // Capacity is 14336, which 2048 less than OmniMap with 10_000 items
    c.bench_function("shrink to fit (HashMap)", |b| {
        let mut map = HashMap::new();
        for i in 0..10_000 {
            map.insert(i.to_string(), i);
        }
        b.iter(|| {
            black_box(map.shrink_to_fit());
        })
    });
}

criterion_group!(
    benches_upsert_get,
    bench_upsert,
    bench_upsert_hashmap,
    bench_get,
    bench_get_hashmap,
    bench_first,
    bench_last,
);

criterion_group!(
    benches_remove_ops,
    bench_remove,
    bench_remove_hashmap,
    bench_pop_first,
    bench_pop_last,
);

criterion_group!(
    benches_shrink_ops,
    bench_shrink_to,
    bench_shrink_to_hashmap,
    bench_shrink_to_fit,
    bench_shrink_to_fit_hashmap,
);

criterion_main!(benches_upsert_get, benches_remove_ops, benches_shrink_ops);