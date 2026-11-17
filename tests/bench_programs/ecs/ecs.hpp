// The ECS library of the C++ half (T-0172): the same design as `libs/ecs` in Dazzle and
// `rust/src/ecs.rs` in Rust. A component type's storage is a sparse set, found by the
// type's id in a map, made the first time an entity is given one; queries walk the smaller
// storage and probe the other. Builds have no RTTI (-fno-rtti), so a type's id is a counter
// a function template hands out, as libraries built without RTTI do.
#pragma once

#include <cstdint>
#include <cstdlib>
#include <memory>
#include <optional>
#include <tuple>
#include <unordered_map>
#include <vector>

namespace ecs {

struct Entity {
    uint32_t index;
    uint32_t generation;
    bool operator==(const Entity& o) const { return index == o.index && generation == o.generation; }
    bool operator!=(const Entity& o) const { return !(*this == o); }
};

inline size_t next_type_id() {
    static size_t next = 0;
    return next++;
}

template <class T> size_t type_id() {
    static const size_t id = next_type_id();
    return id;
}

// What the world knows of a storage without knowing its component type.
struct AnyStorage {
    virtual ~AnyStorage() = default;
    virtual void remove(Entity entity) = 0;
};

template <class T> struct Storage final : AnyStorage {
    std::vector<uint32_t> sparse;  // per entity slot: the row plus one, or zero
    std::vector<Entity> entities;
    std::vector<T> values;

    size_t len() const { return entities.size(); }

    std::optional<size_t> row_of(Entity entity) const {
        size_t index = entity.index;
        if (index >= sparse.size() || sparse[index] == 0) return std::nullopt;
        size_t row = sparse[index] - 1;
        if (entities[row] != entity) return std::nullopt;
        return row;
    }

    T* get(Entity entity) {
        auto row = row_of(entity);
        return row ? &values[*row] : nullptr;
    }

    void insert(Entity entity, T value) {
        size_t index = entity.index;
        if (index >= sparse.size()) sparse.resize(index + 1);
        sparse[index] = static_cast<uint32_t>(entities.size() + 1);
        entities.push_back(entity);
        values.push_back(std::move(value));
    }

    void remove(Entity entity) override {
        auto found = row_of(entity);
        if (!found) return;
        size_t row = *found;
        values[row] = std::move(values.back());
        values.pop_back();
        entities[row] = entities.back();
        entities.pop_back();
        sparse[entity.index] = 0;
        if (row < entities.size()) sparse[entities[row].index] = static_cast<uint32_t>(row + 1);
    }
};

template <class A, class B> class Query2 {
public:
    Query2(Storage<A>& a, Storage<B>& b) : a_(a), b_(b), scan_a_(a.len() <= b.len()) {}

    class iterator {
    public:
        iterator(Query2* q, size_t row) : q_(q), row_(row) { seek(); }
        std::tuple<Entity, A&, B&> operator*() const {
            if (q_->scan_a_) return {q_->a_.entities[row_], q_->a_.values[row_], q_->b_.values[other_]};
            return {q_->b_.entities[row_], q_->a_.values[other_], q_->b_.values[row_]};
        }
        iterator& operator++() {
            ++row_;
            seek();
            return *this;
        }
        bool operator!=(const iterator& o) const { return row_ != o.row_; }

    private:
        // To the first row at or after `row_` whose entity the other storage has too,
        // remembering where.
        void seek() {
            if (q_->scan_a_) {
                for (; row_ < q_->a_.len(); ++row_) {
                    if (auto found = q_->b_.row_of(q_->a_.entities[row_])) {
                        other_ = *found;
                        return;
                    }
                }
            } else {
                for (; row_ < q_->b_.len(); ++row_) {
                    if (auto found = q_->a_.row_of(q_->b_.entities[row_])) {
                        other_ = *found;
                        return;
                    }
                }
            }
        }
        Query2* q_;
        size_t row_;
        size_t other_ = 0;
    };

    iterator begin() { return iterator(this, 0); }
    iterator end() { return iterator(this, scan_a_ ? a_.len() : b_.len()); }

private:
    Storage<A>& a_;
    Storage<B>& b_;
    bool scan_a_;
};

class World;
using System = void (*)(World&);

class World {
public:
    World() { resource_ = spawn(); }

    Entity spawn() {
        if (!free_.empty()) {
            uint32_t index = free_.back();
            free_.pop_back();
            alive_[index] = true;
            return {index, generations_[index]};
        }
        uint32_t index = static_cast<uint32_t>(generations_.size());
        generations_.push_back(0);
        alive_.push_back(true);
        return {index, 0};
    }

    bool is_alive(Entity e) const {
        return e.index < alive_.size() && alive_[e.index] && generations_[e.index] == e.generation;
    }

    void despawn(Entity e) {
        if (!is_alive(e)) return;
        for (auto& s : storages_) s->remove(e);
        alive_[e.index] = false;
        generations_[e.index] += 1;
        free_.push_back(e.index);
    }

    template <class T> Storage<T>& storage() {
        auto found = ids_.find(type_id<T>());
        if (found == ids_.end()) std::abort();  // no entity has this component
        return static_cast<Storage<T>&>(*storages_[found->second]);
    }

    template <class T> void insert(Entity e, T value) {
        storage_or_new<T>().insert(e, std::move(value));
    }

    template <class T> void set_resource(T value) {
        Storage<T>& s = storage_or_new<T>();
        if (T* v = s.get(resource_)) {
            *v = std::move(value);
        } else {
            s.insert(resource_, std::move(value));
        }
    }

    template <class T> T& resource() {
        T* v = storage<T>().get(resource_);
        if (!v) std::abort();
        return *v;
    }

    template <class A, class B> Query2<A, B> query2() { return Query2<A, B>(storage<A>(), storage<B>()); }

    void add_system(System s) { systems_.push_back(s); }

    void step() {
        for (size_t i = 0; i < systems_.size(); ++i) systems_[i](*this);
    }

    size_t slots() const { return generations_.size(); }

    std::optional<Entity> entity_in(size_t index) const {
        if (index >= alive_.size() || !alive_[index]) return std::nullopt;
        return Entity{static_cast<uint32_t>(index), generations_[index]};
    }

private:
    template <class T> Storage<T>& storage_or_new() {
        size_t id = type_id<T>();
        auto found = ids_.find(id);
        if (found != ids_.end()) return static_cast<Storage<T>&>(*storages_[found->second]);
        storages_.push_back(std::make_unique<Storage<T>>());
        ids_.emplace(id, storages_.size() - 1);
        return static_cast<Storage<T>&>(*storages_.back());
    }

    std::vector<uint32_t> generations_;
    std::vector<bool> alive_;
    std::vector<uint32_t> free_;
    std::vector<std::unique_ptr<AnyStorage>> storages_;
    std::unordered_map<size_t, size_t> ids_;
    std::vector<System> systems_;
    Entity resource_{0, 0};
};

}  // namespace ecs
