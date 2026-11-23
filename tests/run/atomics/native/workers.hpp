#pragma once

// Runs `work(id)` on `count` threads at once and returns when all have finished. Dazzle has
// no threads of its own yet (T-0011), so this is what makes the test concurrent.
void run_workers(void (*work)(int), int count);
