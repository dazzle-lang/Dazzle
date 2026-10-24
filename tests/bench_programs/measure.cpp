// Runs a program and reports its wall time and peak memory, for tools/bench_programs.py.
//
// Usage: measure <program> [args...]
// Prints the program's own output as it is, then one line to standard error:
//     measure: <wall seconds> <peak resident KiB>
//
// Why not measure from Python: a process's peak resident size includes the memory of the
// process it was forked from until it calls exec, so a program started by the Python
// runner reports at least the interpreter's ~18 MiB. This helper is small, so the floor
// it adds is the same ~1 MiB for both halves of a pair, as with GNU time. POSIX only; on
// Windows the runner measures wall time itself and reports no memory.
#include <sys/resource.h>
#include <sys/time.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#include <cstdio>

int main(int argc, char** argv) {
    if (argc < 2) {
        std::fprintf(stderr, "usage: measure <program> [args...]\n");
        return 2;
    }
    timespec start{};
    timespec end{};
    clock_gettime(CLOCK_MONOTONIC, &start);
    pid_t pid = fork();
    if (pid < 0) {
        std::perror("fork");
        return 2;
    }
    if (pid == 0) {
        execv(argv[1], argv + 1);
        std::perror("exec");
        _exit(127);
    }
    int status = 0;
    rusage usage{};
    if (wait4(pid, &status, 0, &usage) < 0) {
        std::perror("wait4");
        return 2;
    }
    clock_gettime(CLOCK_MONOTONIC, &end);
    double wall = double(end.tv_sec - start.tv_sec) + double(end.tv_nsec - start.tv_nsec) * 1e-9;
#ifdef __APPLE__
    long peak_kib = long(usage.ru_maxrss / 1024);  // bytes on macOS
#else
    long peak_kib = long(usage.ru_maxrss);  // KiB on Linux and the BSDs
#endif
    std::fprintf(stderr, "measure: %.6f %ld\n", wall, peak_kib);
    if (WIFEXITED(status)) {
        return WEXITSTATUS(status);
    }
    return 128 + (WIFSIGNALED(status) ? WTERMSIG(status) : 0);
}
