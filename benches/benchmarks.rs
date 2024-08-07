use criterion::{black_box, criterion_group, criterion_main, Criterion};
use omni_map::OmniMap;

// black_box is used to prevent the compiler from optimizing the code away

fn bench_upsert(c: &mut Criterion) {
    c.bench_function("upsert 10_000 items", |b| {
        // Without initial capacity
        let mut map = OmniMap::new();
        b.iter(|| {
            for i in 0..10_000 {
                black_box(map.upsert(i.to_string(), i));
            }
        })
    });
}

fn bench_get(c: &mut Criterion) {
    c.bench_function("get item", |b| {
        let mut map = OmniMap::new();
        for i in 0..10_000 {
            map.upsert(i.to_string(), i);
        }
        b.iter(|| {
            black_box(map.get(&"5000".to_string()));
        })
    });
}

fn bench_first(c: &mut Criterion) {
    c.bench_function("get first item", |b| {
        let mut map = OmniMap::new();
        for i in 0..10_000 {
            map.upsert(i.to_string(), i);
        }
        b.iter(|| {
            black_box(map.first());
        })
    });
}

fn bench_last(c: &mut Criterion) {
    c.bench_function("get last item", |b| {
        let mut map = OmniMap::new();
        for i in 0..10_000 {
            map.upsert(i.to_string(), i);
        }
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

criterion_group!(
    benches_upsert_get,
    bench_upsert,
    bench_get,
    bench_first,
    bench_last
);

criterion_group!(
    benches_remove_ops,
    bench_remove,
    bench_pop_first,
    bench_pop_last,
);

criterion_main!(benches_upsert_get, benches_remove_ops);
