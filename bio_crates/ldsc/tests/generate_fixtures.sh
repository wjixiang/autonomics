#!/usr/bin/env bash
# Regenerate the LDSC h² test fixtures from gwas-simulate.
#
# Usage:
#   bash tests/generate_fixtures.sh [path/to/gwas-simulate]
#
# Produces, under tests/fixtures/:
#   sumstats.tsv      – GWAS summary stats (SNP A1 A2 N Z), one replicate
#   ref_ld.l2.ldscore – per-SNP LD Scores (CHR SNP BP LD), single annotation
#   m.l2.M_5_50       – L2-summed M (one float)
#   w_ld.l2.ldscore   – per-SNP weight LD Scores (all ones)
set -euo pipefail

SIM_DIR="${1:-/mnt/disk3/gwas-simulate}"
FIXTURE_DIR="$(cd "$(dirname "$0")" && pwd)/fixtures"
mkdir -p "$FIXTURE_DIR"

cd "$SIM_DIR"
echo ">> running gwas-simulate (this writes to $SIM_DIR/output)"
uv run gwas-simulate \
    --n-snp 1000 --n-sims 5 --h21 0.3 --h22 0.6 --seed 42 \
    -o ./output

echo ">> copying fixtures to $FIXTURE_DIR"
cp output/sumstats/0                          "$FIXTURE_DIR/sumstats.tsv"
cp output/ldscore/oneld_onefile.l2.ldscore    "$FIXTURE_DIR/ref_ld.l2.ldscore"
cp output/ldscore/oneld_onefile.l2.M_5_50     "$FIXTURE_DIR/m.l2.M_5_50"
cp output/ldscore/w.l2.ldscore                "$FIXTURE_DIR/w_ld.l2.ldscore"

echo ">> done. Re-run the integration test with:"
echo "   LDSC_RUN_INTEGRATION=1 cargo test -p ldsc --test integration -- --nocapture"
