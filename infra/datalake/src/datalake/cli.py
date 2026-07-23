from __future__ import annotations

import argparse
import sys

from .catalog import get_catalog, scan_table


def cmd_list(args):
    catalog = get_catalog()
    ns = args.namespace
    if ns:
        tables = catalog.list_tables(ns)
        if not tables:
            print(f"No tables found in namespace '{ns}'")
            return
        for t in tables:
            print(".".join(t))
    else:
        namespaces = catalog.list_namespaces()
        for ns_name in sorted(namespaces):
            tables = catalog.list_tables(ns_name[0])
            if tables:
                for t in tables:
                    print(".".join(t))


def cmd_drop(args):
    catalog = get_catalog()
    table_fqn = args.table

    parts = table_fqn.split(".", 1)
    if len(parts) != 2:
        print(
            f"Error: invalid table name '{table_fqn}', expected format: namespace.table_name"
        )
        sys.exit(1)

    try:
        catalog.load_table(table_fqn)
    except Exception:
        print(f"Error: table '{table_fqn}' does not exist")
        sys.exit(1)

    if not args.yes:
        confirm = input(f"Drop table '{table_fqn}'? [y/N] ").strip().lower()
        if confirm != "y":
            print("Aborted")
            return

    catalog.drop_table(table_fqn)
    print(f"Dropped table '{table_fqn}'")


def cmd_preview(args):
    table_fqn = args.table
    table = scan_table(table_fqn)
    table.show()


def cmd_count(args):
    table_fqn = args.table
    table = scan_table(table_fqn)
    table.count().show()


def main():
    parser = argparse.ArgumentParser(prog="datalake", description="Datalake CLI")
    sub = parser.add_subparsers(dest="command", required=True)

    # List command
    p_list = sub.add_parser("list", help="List tables")
    p_list.add_argument("-n", "--namespace", help="Filter by namespace")
    p_list.set_defaults(func=cmd_list)

    # drop command
    p_drop = sub.add_parser("drop", help="Drop a table")
    p_drop.add_argument(
        "table", help="Fully qualified table name (namespace.table_name)"
    )
    p_drop.add_argument("-y", "--yes", action="store_true", help="Skip confirmation")
    p_drop.set_defaults(func=cmd_drop)

    # preview table command
    p_preview = sub.add_parser(
        "preview", description="Preview table in Iceberg datalake"
    )
    p_preview.add_argument("-t", "--table")
    p_preview.set_defaults(func=cmd_preview)

    # count table command
    p_preview = sub.add_parser(
        "count", description="count table records in Iceberg datalake"
    )
    p_preview.add_argument("-t", "--table")
    p_preview.set_defaults(func=cmd_count)
    args = parser.parse_args()

    args.func(args)


if __name__ == "__main__":
    main()
