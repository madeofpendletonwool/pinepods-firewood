#!/usr/bin/env bash

# Fails if C/C++ code in an x86-64 Linux binary needs more than the baseline CPU.
#
# Libraries choose AVX code at runtime in functions of their own, so the whole binary
# can't be scanned. These functions have no such check: they run on every CPU, and an
# AVX instruction in one kills the process with SIGILL where AVX isn't available.
#
# Usage: check-cpu-baseline.sh <binary>

set -euo pipefail

binary="$1"
required=(
    'signalsmith_stretch_reset'
    'signalsmith_stretch_process'
    'aws_lc_[0-9_]+_CBB_zero'
)
failed=0

for pattern in "${required[@]}"; do
    symbol=$(nm "$binary" | grep -E " [tT] ${pattern}\$" | awk '{print $3}' | head -n1 || true)
    if [ -z "$symbol" ]; then
        echo "error: no function matching $pattern in $binary, so it can't be checked" >&2
        failed=1
        continue
    fi
    count=$(objdump -d --no-show-raw-insn "--disassemble=$symbol" "$binary" |
        grep -cE '[[:space:]]v[a-z0-9]+[[:space:]].*%[xyz]mm' || true)
    echo "$symbol: $count AVX instructions"
    if [ "$count" -ne 0 ]; then
        failed=1
    fi
done

if [ "$failed" -ne 0 ]; then
    echo "error: $binary won't run on every x86-64 CPU" >&2
    exit 1
fi

newest_glibc=$(objdump -T "$binary" | grep -oE 'GLIBC_[0-9.]+' | sort -Vu | tail -n1)
echo "needs $newest_glibc"
