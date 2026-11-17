//! The ECS library of the Rust half (T-0172): the same design as `libs/ecs` in Dazzle and
//! `ecs.hpp` in C++. A component type's storage is a sparse set, found by the type's
//! `TypeId` in a map, made the first time an entity is given one; queries walk the smaller
//! storage and probe the other.
//!
//! What differs is what Rust lets a game write with it. A query borrows the world, so a
//! second query, or a write to another storage, cannot run inside it while it writes:
//! `query2` lends shared references and may nest; `each2_mut` lends mutable ones to one
//! closure at a time; and `storages2_mut` / `storages4_mut` hand out several storages at
//! once, for index loops.

use std::any::{Any, TypeId};
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Entity {
    pub index: u32,
    pub generation: u32,
}

/// What the world knows of a storage without knowing its component type.
pub trait AnyStorage {
    fn remove(&mut self, entity: Entity);
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

pub struct Storage<T> {
    sparse: Vec<u32>, // per entity slot: the row plus one, or zero
    pub entities: Vec<Entity>,
    pub values: Vec<T>,
}

impl<T> Storage<T> {
    fn new() -> Self {
        Storage { sparse: Vec::new(), entities: Vec::new(), values: Vec::new() }
    }

    pub fn len(&self) -> usize {
        self.entities.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    pub fn row_of(&self, entity: Entity) -> Option<usize> {
        let index = entity.index as usize;
        if index >= self.sparse.len() || self.sparse[index] == 0 {
            return None;
        }
        let row = (self.sparse[index] - 1) as usize;
        if self.entities[row] != entity {
            return None;
        }
        Some(row)
    }

    pub fn get(&self, entity: Entity) -> Option<&T> {
        self.row_of(entity).map(|row| &self.values[row])
    }

    pub fn get_mut(&mut self, entity: Entity) -> Option<&mut T> {
        self.row_of(entity).map(|row| &mut self.values[row])
    }

    fn insert(&mut self, entity: Entity, value: T) {
        let index = entity.index as usize;
        if index >= self.sparse.len() {
            self.sparse.resize(index + 1, 0);
        }
        self.sparse[index] = (self.entities.len() + 1) as u32;
        self.entities.push(entity);
        self.values.push(value);
    }
}

impl<T: 'static> AnyStorage for Storage<T> {
    fn remove(&mut self, entity: Entity) {
        let Some(row) = self.row_of(entity) else { return };
        self.values.swap_remove(row);
        self.entities.swap_remove(row);
        self.sparse[entity.index as usize] = 0;
        if row < self.entities.len() {
            let moved = self.entities[row].index as usize;
            self.sparse[moved] = (row + 1) as u32;
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

pub type System = fn(&mut World);

pub struct World {
    generations: Vec<u32>,
    alive: Vec<bool>,
    free: Vec<u32>,
    storages: Vec<Box<dyn AnyStorage>>,
    ids: HashMap<TypeId, usize>,
    systems: Vec<System>,
    resource: Entity,
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

impl World {
    pub fn new() -> Self {
        let mut world = World {
            generations: Vec::new(),
            alive: Vec::new(),
            free: Vec::new(),
            storages: Vec::new(),
            ids: HashMap::new(),
            systems: Vec::new(),
            resource: Entity { index: 0, generation: 0 },
        };
        // Entity 0 holds the world's resources, singleton components of its own.
        world.resource = world.spawn();
        world
    }

    pub fn spawn(&mut self) -> Entity {
        if let Some(index) = self.free.pop() {
            self.alive[index as usize] = true;
            return Entity { index, generation: self.generations[index as usize] };
        }
        let index = self.generations.len() as u32;
        self.generations.push(0);
        self.alive.push(true);
        Entity { index, generation: 0 }
    }

    pub fn is_alive(&self, e: Entity) -> bool {
        let i = e.index as usize;
        i < self.alive.len() && self.alive[i] && self.generations[i] == e.generation
    }

    pub fn despawn(&mut self, e: Entity) {
        if !self.is_alive(e) {
            return;
        }
        for s in &mut self.storages {
            s.remove(e);
        }
        let i = e.index as usize;
        self.alive[i] = false;
        self.generations[i] += 1;
        self.free.push(e.index);
    }

    fn index_of<T: 'static>(&self) -> usize {
        *self.ids.get(&TypeId::of::<T>()).expect("no entity has this component")
    }

    pub fn storage<T: 'static>(&self) -> &Storage<T> {
        self.storages[self.index_of::<T>()].as_any().downcast_ref().unwrap()
    }

    pub fn storage_mut<T: 'static>(&mut self) -> &mut Storage<T> {
        let i = self.index_of::<T>();
        self.storages[i].as_any_mut().downcast_mut().unwrap()
    }

    /// Two different storages at once, both writable.
    pub fn storages2_mut<A: 'static, B: 'static>(&mut self) -> (&mut Storage<A>, &mut Storage<B>) {
        let (i, j) = (self.index_of::<A>(), self.index_of::<B>());
        let [a, b] = self.storages.get_disjoint_mut([i, j]).expect("two different components");
        (a.as_any_mut().downcast_mut().unwrap(), b.as_any_mut().downcast_mut().unwrap())
    }

    /// Four different storages at once, all writable.
    #[allow(clippy::type_complexity)]
    pub fn storages4_mut<A: 'static, B: 'static, C: 'static, D: 'static>(
        &mut self,
    ) -> (&mut Storage<A>, &mut Storage<B>, &mut Storage<C>, &mut Storage<D>) {
        let ids = [
            self.index_of::<A>(),
            self.index_of::<B>(),
            self.index_of::<C>(),
            self.index_of::<D>(),
        ];
        let [a, b, c, d] = self.storages.get_disjoint_mut(ids).expect("four different components");
        (
            a.as_any_mut().downcast_mut().unwrap(),
            b.as_any_mut().downcast_mut().unwrap(),
            c.as_any_mut().downcast_mut().unwrap(),
            d.as_any_mut().downcast_mut().unwrap(),
        )
    }

    fn storage_or_new<T: 'static>(&mut self) -> &mut Storage<T> {
        let id = TypeId::of::<T>();
        let i = match self.ids.get(&id) {
            Some(&i) => i,
            None => {
                self.storages.push(Box::new(Storage::<T>::new()));
                self.ids.insert(id, self.storages.len() - 1);
                self.storages.len() - 1
            }
        };
        self.storages[i].as_any_mut().downcast_mut().unwrap()
    }

    pub fn insert<T: 'static>(&mut self, e: Entity, value: T) {
        self.storage_or_new::<T>().insert(e, value);
    }

    pub fn set_resource<T: 'static>(&mut self, value: T) {
        let r = self.resource;
        let s = self.storage_or_new::<T>();
        match s.get_mut(r) {
            Some(v) => *v = value,
            None => s.insert(r, value),
        }
    }

    pub fn resource<T: 'static>(&self) -> &T {
        self.storage::<T>().get(self.resource).expect("no such resource")
    }

    pub fn resource_mut<T: 'static>(&mut self) -> &mut T {
        let r = self.resource;
        self.storage_mut::<T>().get_mut(r).expect("no such resource")
    }

    /// Every entity with an `A` and a `B`, shared: queries of this kind may nest.
    pub fn query2<A: 'static, B: 'static>(&self) -> impl Iterator<Item = (Entity, &A, &B)> {
        let (a, b) = (self.storage::<A>(), self.storage::<B>());
        let scan_a = a.len() <= b.len();
        let rows = if scan_a { a.len() } else { b.len() };
        (0..rows).filter_map(move |row| {
            if scan_a {
                let e = a.entities[row];
                b.row_of(e).map(|rb| (e, &a.values[row], &b.values[rb]))
            } else {
                let e = b.entities[row];
                a.row_of(e).map(|ra| (e, &a.values[ra], &b.values[row]))
            }
        })
    }

    /// Every entity with an `A` and a `B`, both writable, handed to `f` one at a time.
    pub fn each2_mut<A: 'static, B: 'static>(&mut self, mut f: impl FnMut(Entity, &mut A, &mut B)) {
        let (a, b) = self.storages2_mut::<A, B>();
        if a.len() <= b.len() {
            for row in 0..a.len() {
                let e = a.entities[row];
                if let Some(rb) = b.row_of(e) {
                    f(e, &mut a.values[row], &mut b.values[rb]);
                }
            }
        } else {
            for row in 0..b.len() {
                let e = b.entities[row];
                if let Some(ra) = a.row_of(e) {
                    f(e, &mut a.values[ra], &mut b.values[row]);
                }
            }
        }
    }

    pub fn add_system(&mut self, system: System) {
        self.systems.push(system);
    }

    pub fn step(&mut self) {
        for i in 0..self.systems.len() {
            let system = self.systems[i];
            system(self);
        }
    }

    pub fn slots(&self) -> usize {
        self.generations.len()
    }

    pub fn entity_in(&self, index: usize) -> Option<Entity> {
        if index >= self.alive.len() || !self.alive[index] {
            return None;
        }
        Some(Entity { index: index as u32, generation: self.generations[index] })
    }
}
