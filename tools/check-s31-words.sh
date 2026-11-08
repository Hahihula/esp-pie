#!/bin/sh
# Checks the raw xespv instruction words in src/esp32s31.rs against
# Espressif's GNU assembler: each `.4byte 0x...` line carries its
# instruction as a comment; this assembles those instructions and compares
# the bytes. Needs riscv32-esp-elf-as with xespv support (ESP-IDF's GCC 14+);
# set AS=/path/to/riscv32-esp-elf-as if it isn't on PATH.
set -eu
cd "$(dirname "$0")/.."
AS=${AS:-riscv32-esp-elf-as}
OBJCOPY=${OBJCOPY:-$(dirname "$(command -v "$AS")")/riscv32-esp-elf-objcopy}
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# `".4byte 0x0311009f", // esp.vld.128.ip q0, a0, 16` -> word and instruction.
sed -n 's/.*"\.4byte \(0x[0-9a-fA-F]*\)", *\/\/ *\(.*\)$/\1|\2/p' src/esp32s31.rs > "$tmp/pairs"
[ -s "$tmp/pairs" ] || { echo "no .4byte lines found"; exit 1; }
cut -d'|' -f2 "$tmp/pairs" > "$tmp/k.s"
"$AS" -march=rv32imafc_xespv -o "$tmp/k.o" "$tmp/k.s"
"$OBJCOPY" -O binary -j .text "$tmp/k.o" "$tmp/k.bin"

n=0; bad=0
od -An -v -tx4 "$tmp/k.bin" | tr -s ' ' '\n' | sed '/^$/d' > "$tmp/got"
while IFS='|' read -r want insn; do
  n=$((n + 1))
  got=0x$(sed -n "${n}p" "$tmp/got")
  if [ "$(printf '%d' "$want")" -ne "$(printf '%d' "$got")" ]; then
    echo "MISMATCH line $n: $insn: source $want, assembler $got"
    bad=$((bad + 1))
  fi
done < "$tmp/pairs"
echo "$n words checked, $bad mismatches"
[ "$bad" -eq 0 ]
