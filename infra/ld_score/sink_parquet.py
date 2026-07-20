"""Ingest a local Parquet file of LD-score panel data into Iceberg.

Reads a Spark-produced Parquet (where nested ``locus`` structs are flattened
into dot-delimited columns like ``locus.contig`` / ``locus.position``), rebuilds
the proper struct, and appends the data to an Iceberg table in the
``ld_score`` namespace.

Usage
-----
    python sink_parquet.py --parquet-path ./UKBB.EUR.ldscore.parquet --table-name ukbb_eur
    python sink_parquet.py --parquet-path ./UKBB.AFR.ldscore.parquet --table-name ukbb_afr --mode overwrite
"""

from __future__ import annotations

import argparse
import os
import sys

import pyarrow as pa
import pyarrow.parquet as pq
from pyiceberg.catalog import load_catalog

from config import get_catalog
from schema import LD_SCORE_SCHEMA

# ---------------------------------------------------------------------------
# Defaults (overridable via CLI / env)
# ---------------------------------------------------------------------------
TABLE_NAMESPACE = os.environ.get("LD_SCORE_NAMESPACE", "ld_score")


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
def read_parquet(path: str) -> pa.Table:
    """Read a local Parquet directory into a PyArrow Table."""
    dataset = pq.ParquetDataset(path)
    return dataset.read()


def create_or_load_table(
    catalog: object, full_table: str, schema: object = LD_SCORE_SCHEMA
):
    """Create the Iceberg table or load an existing one.

    If the table exists but its schema has drifted, it is dropped and
    recreated.
    """
    if catalog.table_exists(full_table):
        existing = catalog.load_table(full_table)
        if existing.schema() == schema:
            print(f"Loading existing table: {full_table}")
            return existing
        print(f"Schema mismatch, dropping and recreating: {full_table}")
        catalog.drop_table(full_table)

    print(f"Creating table: {full_table}")
    return catalog.create_table(full_table, schema=schema)


def rebuild_locus_struct(arrow_table: pa.Table) -> pa.Table:
    """Rebuild the nested ``locus`` struct from Spark-flattened columns.

    Spark writes ``locus.contig`` and ``locus.position`` as flat top-level
    columns.  Iceberg expects a proper ``locus: struct<contig, position>``
    column.  This function drops the flat columns and prepends the struct.
    """
    locus = pa.StructArray.from_arrays(
        [
            arrow_table.column("locus.contig").combine_chunks(),
            arrow_table.column("locus.position").combine_chunks(),
        ],
        names=["contig", "position"],
    )

    return (
        arrow_table.drop_columns(["locus.contig", "locus.position"])
        .add_column(
            0,
            pa.field(
                "locus",
                pa.struct(
                    [pa.field("contig", pa.string()), pa.field("position", pa.int32())]
                ),
            ),
            locus,
        )
    )


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------
def main() -> None:
    parser = argparse.ArgumentParser(
        description="Ingest LD-score Parquet into Iceberg"
    )
    parser.add_argument(
        "--parquet-path",
        required=True,
        help="Path to the local Parquet directory to ingest",
    )
    parser.add_argument(
        "--table-name",
        required=True,
        help="Iceberg table name (e.g. ukbb_eur)",
    )
    parser.add_argument(
        "--namespace",
        default=TABLE_NAMESPACE,
        help=f"Iceberg namespace (default: {TABLE_NAMESPACE})",
    )
    parser.add_argument(
        "--mode",
        choices=["append", "overwrite"],
        default="append",
        help="Write mode (default: append)",
    )
    args = parser.parse_args()

    full_table = f"{args.namespace}.{args.table_name}"

    # ---- catalog ----
    catalog = get_catalog()
    print("Catalog loaded:", catalog.name)
    print("Namespaces:", catalog.list_namespaces())

    catalog.create_namespace_if_not_exists(namespace=args.namespace)

    # ---- read parquet ----
    arrow_table = read_parquet(args.parquet_path)
    print(
        f"Parquet: {arrow_table.num_rows} rows x {len(arrow_table.column_names)} cols"
    )

    # ---- rebuild nested structs (Spark compatibility) ----
    if "locus.contig" in arrow_table.column_names:
        print("Detected Spark-flattened locus columns — rebuilding struct")
        arrow_table = rebuild_locus_struct(arrow_table)

    print(f"Columns: {arrow_table.column_names}")
    print(f"Schema:  {arrow_table.schema}")

    # ---- iceberg table ----
    iceberg_table = create_or_load_table(catalog, full_table)

    # ---- write ----
    iceberg_table.append(arrow_table)
    print(f"Appended to {full_table}")
    print("Snapshot history:", iceberg_table.snapshots())


if __name__ == "__main__":
    main()
