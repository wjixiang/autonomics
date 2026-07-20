"""Export a Hail Table of LD scores to coalesced Parquet.

This script requires `hail <https://hail.is>` to be installed in the
active Python environment.  It is **not** declared as a dependency of
``infra/pyproject.toml`` because Hail bundles its own Spark runtime and
is typically installed separately.

Usage
-----
    python main.py --input ../UKBB.AFR.ldscore.ht --output ./UKBB.AFR.ldscore.parquet
"""

from __future__ import annotations

import argparse


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Export a Hail LD-score Table to Parquet"
    )
    parser.add_argument(
        "--input",
        required=True,
        help="Path to the Hail Table (.ht) to read",
    )
    parser.add_argument(
        "--output",
        required=True,
        help="Output Parquet directory path",
    )
    args = parser.parse_args()

    import hail as hl  # noqa: delayed import — hail is an optional runtime dep

    ht = hl.read_table(args.input)
    print(ht.show())
    df = ht.to_spark()
    df.coalesce(1).write.parquet(args.output)
    print(f"Written to {args.output}")


if __name__ == "__main__":
    main()
