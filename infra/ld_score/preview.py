"""List and preview LD-score tables in the Iceberg datalake.

Usage
-----
    python preview.py                        # list tables in ld_score namespace
    python preview.py --namespace ld_score --table ukbb_eur --scan  # scan to pandas
"""

from __future__ import annotations

import argparse

from config import get_catalog


def main() -> None:
    parser = argparse.ArgumentParser(
        description="List / preview LD-score Iceberg tables"
    )
    parser.add_argument(
        "--namespace",
        default="ld_score",
        help="Iceberg namespace to list (default: ld_score)",
    )
    parser.add_argument(
        "--table",
        default=None,
        help="Table name to scan (if set, loads the table)",
    )
    parser.add_argument(
        "--scan",
        action="store_true",
        help="If --table is set, scan the table to pandas and print",
    )
    args = parser.parse_args()

    catalog = get_catalog()
    print("Catalog:", catalog.name)
    print(f"Tables in '{args.namespace}':", catalog.list_tables(args.namespace))

    if args.table:
        full = f"{args.namespace}.{args.table}"
        table = catalog.load_table(full)
        print(f"\nTable: {full}")
        print("  Schema:", table.schema())
        print("  Snapshots:", table.snapshots())
        if args.scan:
            df = table.scan().to_pandas()
            print(f"\n{len(df)} rows:")
            print(df.head())
    else:
        print(
            "\nTip: use --table <name> [--scan] to inspect a specific table"
        )


if __name__ == "__main__":
    main()
