/* The reference workload, written in C.
 *
 * This is the baseline for bench/run.sh. The Rust version in bench/rust does
 * the same arithmetic in the same order, so the two rows of the table are
 * comparable.
 *
 * The algorithm is the sieve of Eratosthenes followed by a sum over a matrix,
 * chosen because it is compute bound, easy to verify, and not dependent on
 * anything the Vortex runtime might special case. */

#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>

#define LIMIT 2000000
#define N 256

static uint32_t sieve_sum(uint32_t limit) {
    if (limit < 2) {
        return 0;
    }

    uint8_t *composite = (uint8_t *)calloc((size_t)limit + 1, sizeof(uint8_t));
    if (composite == NULL) {
        fprintf(stderr, "sieve: out of memory\n");
        exit(1);
    }

    uint32_t total = 0;
    for (uint32_t i = 2; i <= limit; i++) {
        if (composite[i]) {
            continue;
        }
        total += i;
        if ((uint64_t)i * i <= limit) {
            for (uint64_t j = (uint64_t)i * i; j <= limit; j += i) {
                composite[j] = 1;
            }
        }
    }

    free(composite);
    return total;
}

static double matrix_work(void) {
    static double a[N][N];
    static double b[N][N];
    static double c[N][N];

    for (int i = 0; i < N; i++) {
        for (int j = 0; j < N; j++) {
            a[i][j] = (double)((i + j) % 17);
            b[i][j] = (double)((i * 3 + j * 5) % 23);
        }
    }

    /* The inner loop is deliberately simple so the compiler cannot hoist it
     * out of the timed region and leave the benchmark measuring nothing. */
    for (int k = 0; k < 8; k++) {
        for (int i = 0; i < N; i++) {
            for (int j = 0; j < N; j++) {
                c[i][j] = a[i][j] + b[i][j];
            }
        }
        for (int i = 0; i < N; i++) {
            for (int j = 0; j < N; j++) {
                a[i][j] = c[i][j] * 0.5 + 1.0;
            }
        }
    }

    double total = 0.0;
    for (int i = 0; i < N; i++) {
        total += a[i][i];
    }
    return total;
}

int main(void) {
    uint32_t s = sieve_sum(LIMIT);
    double m = matrix_work();

    /* The checksum is printed so a run cannot be optimised away, and so a
     * Vortex implementation can be checked against this one. */
    printf("checksum %u %.6f\n", s, m);
    return 0;
}