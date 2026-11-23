#include "worker.hpp"

#include <array>
#include <thread>

void run_worker(void (*work)(int), int id) {
    std::thread worker(work, id);
    worker.join();
}

void exchange_allocations(void* (*allocate)(int), void (*release)(void*, int)) {
    constexpr int worker_count = 4;
    constexpr int block_count = 512;
    std::array<std::array<void*, block_count>, worker_count> blocks{};
    std::array<std::thread, worker_count> workers;

    for (int id = 0; id < worker_count; ++id) {
        workers[id] = std::thread([&, id] {
            for (int index = 0; index < block_count; ++index) {
                release(allocate(index), index);
                blocks[id][index] = allocate(index);
            }
        });
    }
    for (auto& worker : workers) {
        worker.join();
    }

    for (int id = 0; id < worker_count; ++id) {
        workers[id] = std::thread([&, id] {
            const auto& allocated = blocks[(id + 1) % worker_count];
            for (int index = 0; index < block_count; ++index) {
                release(allocated[index], index);
            }
        });
    }
    for (auto& worker : workers) {
        worker.join();
    }
}
