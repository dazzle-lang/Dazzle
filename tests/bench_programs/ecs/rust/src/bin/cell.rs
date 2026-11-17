//! The battle, with interior mutability: the components a nested loop writes, velocity and
//! health, are stored as `Cell`s, so every query borrows the world shared, they nest as they
//! do in Dazzle, and a write is a `get` and a `set` of the whole component.

use std::cell::Cell;

use ecs_bench::ecs::{Entity, World};
use ecs_bench::*;

fn spawn_unit(world: &mut World, rng: &mut Rng, side: u32) {
    let (p, v, h) = draw_unit(rng);
    let e = world.spawn();
    world.insert(e, p);
    world.insert(e, Cell::new(v));
    world.insert(e, Team { side });
    world.insert(e, Cell::new(h));
}

fn movement(world: &mut World) {
    world.each2_mut::<Position, Cell<Velocity>>(|_, p, v| {
        let v = v.get_mut();
        v.x *= FRICTION;
        v.y *= FRICTION;
        p.x += v.x;
        p.y += v.y;
        bounce(&mut p.x, &mut v.x);
        bounce(&mut p.y, &mut v.y);
    });
}

fn separate(world: &mut World) {
    for (a, pa, va) in world.query2::<Position, Cell<Velocity>>() {
        for (b, pb, vb) in world.query2::<Position, Cell<Velocity>>() {
            if a.index < b.index {
                let dx = pa.x - pb.x;
                let dy = pa.y - pb.y;
                let d2 = dx * dx + dy * dy;
                if d2 < PERSONAL {
                    let s = (PERSONAL - d2) * PUSH;
                    let mut v = va.get();
                    v.x += dx * s;
                    v.y += dy * s;
                    va.set(v);
                    let mut v = vb.get();
                    v.x -= dx * s;
                    v.y -= dy * s;
                    vb.set(v);
                }
            }
        }
    }
}

fn fight(world: &mut World) {
    let health = world.storage::<Cell<Health>>();
    let velocity = world.storage::<Cell<Velocity>>();
    for (a, pa, ta) in world.query2::<Position, Team>() {
        let mut best: Option<Entity> = None;
        let (mut best_d2, mut bx, mut by) = (0.0f32, 0.0f32, 0.0f32);
        for (b, pb, tb) in world.query2::<Position, Team>() {
            if tb.side != ta.side {
                let dx = pb.x - pa.x;
                let dy = pb.y - pa.y;
                let d2 = dx * dx + dy * dy;
                if best.is_none() || d2 < best_d2 {
                    best = Some(b);
                    best_d2 = d2;
                    bx = dx;
                    by = dy;
                }
            }
        }
        let Some(target) = best else { continue };
        if best_d2 < REACH {
            let h = health.get(target).unwrap();
            h.set(Health { hp: h.get().hp - 1 });
        } else {
            let v = velocity.get(a).unwrap();
            let mut w = v.get();
            w.x += bx * STEER;
            w.y += by * STEER;
            v.set(w);
        }
    }
}

fn reinforce(world: &mut World) {
    let mut dead = Vec::new();
    for (e, h, t) in world.query2::<Cell<Health>, Team>() {
        if h.get().hp <= 0 {
            dead.push((e, t.side));
        }
    }
    let mut rng = *world.resource::<Rng>();
    for &(e, side) in &dead {
        world.despawn(e);
        spawn_unit(world, &mut rng, side);
    }
    world.set_resource(rng);
    world.resource_mut::<Fallen>().count += dead.len() as u64;
}

fn main() {
    let (units, steps) = args();
    let mut world = World::new();
    let mut rng = Rng { state: 42 };
    for i in 0..units {
        spawn_unit(&mut world, &mut rng, (i % 2) as u32);
    }
    world.set_resource(rng);
    world.set_resource(Fallen { count: 0 });
    world.add_system(movement);
    world.add_system(separate);
    world.add_system(fight);
    world.add_system(reinforce);
    for _ in 0..steps {
        world.step();
    }
    report(&world, units, steps, |w, e| w.storage::<Cell<Health>>().get(e).unwrap().get().hp);
}
