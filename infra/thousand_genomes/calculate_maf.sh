#!/bin/bash

OUT_ROOT="/mnt/disk2/dataset/1000g_plink"
pop="EUR"

process_chr() {
  local pop="$1" chr="$2"
  local pop_dir="$OUT_ROOT/${pop,,}"
  local chr_dir="$pop_dir/chr${chr}"
  local maf_dir="$pop_dir/maf"

  mkdir -p "$maf_dir"

  plink2 --bfile "${chr_dir}/1000G.EUR.chr$2.qc" \
    --freq --threads 12 \
    --out "${maf_dir}/1000G.EUR.chr$2.qc"
}

for chr in $(seq 1 22); do
  process_chr "$pop" "$chr"
done
