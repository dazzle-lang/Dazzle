#include "workers.hpp"

#include <thread>
#include <vector>

void run_workers(void (*work)(int), int count) {
    std::vector<std::thread> workers;
    workers.reserve(static_cast<size_t>(count));
    for (int id = 0; id < count; ++id) {
        workers.emplace_back(work, id);
    }
    for (auto& worker : workers) {
        worker.join();
    }
}
