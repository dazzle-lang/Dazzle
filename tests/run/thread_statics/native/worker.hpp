#pragma once

void run_worker(void (*work)(int), int id);
void exchange_allocations(void* (*allocate)(int), void (*release)(void*, int));
