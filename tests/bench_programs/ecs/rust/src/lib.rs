//! The Rust half of `ecs` (T-0172): the library in `ecs.rs`, and what the three versions of
//! the battle share. Each version is a binary in `src/bin/`, and differs from the others
//! only in how it writes the two systems that matter, `separate` and `fight`.

pub mod ecs;

use ecs::{Entity, World};

pub const ARENA: f32 = 800.0;
/// Units closer than this, squared, push each other apart.
pub const PERSONAL: f32 = 64.0;
pub const PUSH: f32 = 0.01;
/// A unit strikes an enemy closer than this, squared.
pub const REACH: f32 = 900.0;
pub const STEER: f32 = 0.0005;
pub const FRICTION: f32 = 0.98;

#[derive(Clone, Copy)]
pub struct Position {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy)]
pub struct Velocity {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy)]
pub struct Team {
    pub side: u32,
}

#[derive(Clone, Copy)]
pub struct Health {
    pub hp: i32,
}

/// How many units have fallen, a resource.
pub struct Fallen {
    pub count: u64,
}

/// SplitMix64, as every benchmark here generates its input.
#[derive(Clone, Copy)]
pub struct Rng {
    pub state: u64,
}

impl Rng {
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }

    /// A number in [0, 1) with 24 bits, so `f32` holds it exactly in every language.
    pub fn unit(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / 16777216.0
    }

    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }
}

/// A new unit's position, velocity and health, drawn in the order every half draws them.
pub fn draw_unit(rng: &mut Rng) -> (Position, Velocity, Health) {
    let x = rng.unit() * ARENA;
    let y = rng.unit() * ARENA;
    let vx = rng.unit() - 0.5;
    let vy = rng.unit() - 0.5;
    let hp = 50 + rng.below(50) as i32;
    (Position { x, y }, Velocity { x: vx, y: vy }, Health { hp })
}

pub fn bounce(p: &mut f32, v: &mut f32) {
    if *p < 0.0 {
        *p = -*p;
        *v = -*v;
    }
    if *p > ARENA {
        *p = 2.0 * ARENA - *p;
        *v = -*v;
    }
}

/// The units and steps asked for on the command line, 600 and 100 by default.
pub fn args() -> (i64, i64) {
    let args: Vec<String> = std::env::args().collect();
    let units = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(600);
    let steps = args.get(2).and_then(|a| a.parse().ok()).unwrap_or(100);
    (units, steps)
}

/// Every live unit, in slot order, folded as every half folds it; `hp` reads a unit's
/// health from wherever the version keeps it.
pub fn report(world: &World, units: i64, steps: i64, hp_of: impl Fn(&World, Entity) -> i32) {
    let mut checksum: u64 = 1469598103934665603;
    let mut alive: u64 = 0;
    let mut hp: i64 = 0;
    for i in 0..world.slots() {
        let Some(e) = world.entity_in(i) else { continue };
        let Some(p) = world.storage::<Position>().get(e) else { continue };
        let h = hp_of(world, e);
        let t = world.storage::<Team>().get(e).unwrap();
        let values = [
            e.index as u64,
            e.generation as u64,
            t.side as u64,
            h as u64,
            (p.x * 1024.0) as i64 as u64,
            (p.y * 1024.0) as i64 as u64,
        ];
        for v in values {
            checksum = (checksum ^ v).wrapping_mul(1099511628211);
        }
        alive += 1;
        hp += h as i64;
    }
    let fallen = world.resource::<Fallen>().count;
    println!("{units} units, {steps} steps: {alive} alive, {fallen} fallen, {hp} hp");
    println!("checksum {checksum}");
}
