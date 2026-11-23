#include "worker.hpp"

#include <thread>

void run_worker(void (*work)(int), int id) {
    std::thread worker(work, id);
    worker.join();
}
