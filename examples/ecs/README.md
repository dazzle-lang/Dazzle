# Sparse-set ECS

A headless example of entities, component pools, and queries. It needs only the Dazzle standard library.

```sh
dz run examples/ecs
dz test examples/ecs
```

`World` keeps typed pools for `Position`, `Velocity`, and `Health`. Each pool has dense arrays for keys and component values, plus a sparse array that maps an entity index back to its dense row. Removing a component moves the last row into the gap and repairs the sparse index. Entity keys carry a generation so a recycled slot cannot bring an old key back to life.

```dz
for _, &mut position, &velocity in query2(&mut world.positions, &mut world.velocities) {
    position.x += velocity.x
}
```

Use `query` for one pool and `query2` to join two pools by entity. The join scans the smaller pool, probes the other, and yields references to matching components without collecting a result list. The collision system nests two queries and updates both entities in place. The cleanup system collects dead entity keys first, then despawns them after the query has finished. Keep spawns, despawns, and component insertion or removal outside active query loops.

The demo prints the surviving moving entities after one tick and checks that reusing an entity slot leaves the old key invalid.
