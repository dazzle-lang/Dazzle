//! The battle, collect-then-apply: the nested queries borrow the world shared, so what they
//! decide is gathered into a list and written after the loops. The usual answer in Rust.

use ecs_bench::ecs::{Entity, World};
use ecs_bench::*;

fn spawn_unit(world: &mut World, rng: &mut Rng, side: u32) {
    let (p, v, h) = draw_unit(rng);
    let e = world.spawn();
    world.insert(e, p);
    world.insert(e, v);
    world.insert(e, Team { side });
    world.insert(e, h);
}

fn movement(world: &mut World) {
    world.each2_mut::<Position, Velocity>(|_, p, v| {
        v.x *= FRICTION;
        v.y *= FRICTION;
        p.x += v.x;
        p.y += v.y;
        bounce(&mut p.x, &mut v.x);
        bounce(&mut p.y, &mut v.y);
    });
}

fn separate(world: &mut World) {
    let mut pushes: Vec<(Entity, Entity, f32, f32)> = Vec::new();
    for (a, pa, _) in world.query2::<Position, Velocity>() {
        for (b, pb, _) in world.query2::<Position, Velocity>() {
            if a.index < b.index {
                let dx = pa.x - pb.x;
                let dy = pa.y - pb.y;
                let d2 = dx * dx + dy * dy;
                if d2 < PERSONAL {
                    let s = (PERSONAL - d2) * PUSH;
                    pushes.push((a, b, dx * s, dy * s));
                }
            }
        }
    }
    let velocity = world.storage_mut::<Velocity>();
    for (a, b, px, py) in pushes {
        let va = velocity.get_mut(a).unwrap();
        va.x += px;
        va.y += py;
        let vb = velocity.get_mut(b).unwrap();
        vb.x -= px;
        vb.y -= py;
    }
}

enum Act {
    Strike(Entity),
    Close(Entity, f32, f32),
}

fn fight(world: &mut World) {
    let mut acts: Vec<Act> = Vec::new();
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
            acts.push(Act::Strike(target));
        } else {
            acts.push(Act::Close(a, bx, by));
        }
    }
    let (health, velocity) = world.storages2_mut::<Health, Velocity>();
    for act in acts {
        match act {
            Act::Strike(target) => health.get_mut(target).unwrap().hp -= 1,
            Act::Close(a, bx, by) => {
                let v = velocity.get_mut(a).unwrap();
                v.x += bx * STEER;
                v.y += by * STEER;
            }
        }
    }
}

fn reinforce(world: &mut World) {
    let mut dead = Vec::new();
    for (e, h, t) in world.query2::<Health, Team>() {
        if h.hp <= 0 {
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
    report(&world, units, steps, |w, e| w.storage::<Health>().get(e).unwrap().hp);
}
