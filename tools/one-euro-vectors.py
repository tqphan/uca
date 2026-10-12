# SPDX-FileCopyrightText: The UCA contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Generate One-Euro filter test vectors (03 "Acceptance tests").

An implementation independent of the Rust one, written from the equations of
Casiez, Roussel and Vogel (CHI 2012) as in the authors' reference C++ code:
the speed is estimated from the last filtered value, the rate from timestamps.

The input is deterministic (a sine plus pseudo-random noise from a fixed LCG,
with uneven sample spacing), so the output never changes between runs.

Usage: python tools/one-euro-vectors.py > crates/uca-input/tests/data/one_euro.csv
"""

import math

PARAMS = [(1.0, 0.1, 1.0), (1.0, 0.007, 1.0), (0.5, 0.5, 2.0)]
SAMPLES = 400


def lcg(seed):
    state = seed
    while True:
        state = (state * 6364136223846793005 + 1442695040888963407) % 2**64
        yield (state >> 11) / float(2**53) - 0.5


def alpha(cutoff, rate):
    te = 1.0 / rate
    tau = 1.0 / (2.0 * math.pi * cutoff)
    return 1.0 / (1.0 + tau / te)


def one_euro(samples, min_cutoff, beta, d_cutoff):
    out = []
    x_prev = None
    dx_prev = 0.0
    t_prev = None
    for t, x in samples:
        if t_prev is None:
            x_hat, dx_hat = x, 0.0
        else:
            rate = 1.0 / (t - t_prev)
            dx = (x - x_prev) * rate
            a_d = alpha(d_cutoff, rate)
            dx_hat = a_d * dx + (1.0 - a_d) * dx_prev
            cutoff = min_cutoff + beta * abs(dx_hat)
            a = alpha(cutoff, rate)
            x_hat = a * x + (1.0 - a) * x_prev
        out.append(x_hat)
        x_prev, dx_prev, t_prev = x_hat, dx_hat, t
    return out


def main():
    print("# One-Euro test vectors from tools/one-euro-vectors.py; do not edit.")
    print("min_cutoff,beta,d_cutoff,t,x,filtered")
    for i, (min_cutoff, beta, d_cutoff) in enumerate(PARAMS):
        noise = lcg(42 + i)
        t = 0.0
        samples = []
        for n in range(SAMPLES):
            x = 100.0 * math.sin(2.0 * math.pi * 0.5 * t) + 5.0 * next(noise)
            samples.append((t, x))
            t += (1.0 / 120.0) * (1.0 + 0.3 * math.sin(n))
        for (t, x), y in zip(samples, one_euro(samples, min_cutoff, beta, d_cutoff)):
            print(f"{min_cutoff!r},{beta!r},{d_cutoff!r},{t!r},{x!r},{y!r}")


if __name__ == "__main__":
    main()
