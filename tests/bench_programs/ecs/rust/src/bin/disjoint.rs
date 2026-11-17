//! The battle, in index loops: the storages each system needs are taken from the world at
//! once (`get_disjoint_mut` on the world's list of them), and the loops walk rows, taking two
//! velocities at once with `get_disjoint_mut` where one unit writes another's. The query is
//! written out by hand: each loop walks `Position`'s rows and finds the other component's
//! row, which is what `query2` does when, as here, both storages are the same size.

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
    let (position, velocity) = world.storages2_mut::<Position, Velocity>();
    for ra in 0..position.len() {
        let a = position.entities[ra];
        let Some(va) = velocity.row_of(a) else { continue };
        for rb in 0..position.len() {
            let b = position.entities[rb];
            let Some(vb) = velocity.row_of(b) else { continue };
            if a.index < b.index {
                let (pa, pb) = (position.values[ra], position.values[rb]);
                let dx = pa.x - pb.x;
                let dy = pa.y - pb.y;
                let d2 = dx * dx + dy * dy;
                if d2 < PERSONAL {
                    let s = (PERSONAL - d2) * PUSH;
                    let [va, vb] = velocity.values.get_disjoint_mut([va, vb]).unwrap();
                    va.x += dx * s;
                    va.y += dy * s;
                    vb.x -= dx * s;
                    vb.y -= dy * s;
                }
            }
        }
    }
}

fn fight(world: &mut World) {
    let (position, team, health, velocity) =
        world.storages4_mut::<Position, Team, Health, Velocity>();
    for ra in 0..position.len() {
        let a = position.entities[ra];
        let Some(ta) = team.get(a) else { continue };
        let pa = position.values[ra];
        let mut best: Option<Entity> = None;
        let (mut best_d2, mut bx, mut by) = (0.0f32, 0.0f32, 0.0f32);
        for rb in 0..position.len() {
            let b = position.entities[rb];
            let Some(tb) = team.get(b) else { continue };
            if tb.side != ta.side {
                let pb = position.values[rb];
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
            health.get_mut(target).unwrap().hp -= 1;
        } else {
            let v = velocity.get_mut(a).unwrap();
            v.x += bx * STEER;
            v.y += by * STEER;
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
