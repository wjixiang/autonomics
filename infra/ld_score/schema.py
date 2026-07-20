"""Canonical Iceberg schema for LD-score panel tables.

This is the single source of truth for the column layout that every
LD-score Iceberg table must conform to.  Both the ingestion script
(`sink_parquet.py`) and Rust consumers (`data-engine/nodes/ldsc_hsq.rs`)
expect this shape.

Field map
---------
locus       Struct { contig: string, position: int32 }
alleles     List<string>
rsid        string
AF          float64    — allele frequency
ld_score    float64    — LD score value
"""

from pyiceberg.schema import Schema
from pyiceberg.types import (
    DoubleType,
    IntegerType,
    ListType,
    NestedField,
    StringType,
    StructType,
)

LD_SCORE_SCHEMA = Schema(
    NestedField(
        1,
        "locus",
        StructType(
            NestedField(2, "contig", StringType(), required=False),
            NestedField(3, "position", IntegerType(), required=False),
        ),
        required=False,
    ),
    NestedField(
        4, "alleles", ListType(5, StringType(), element_required=False), required=False
    ),
    NestedField(6, "rsid", StringType(), required=False),
    NestedField(7, "AF", DoubleType(), required=False),
    NestedField(8, "ld_score", DoubleType(), required=False),
)
