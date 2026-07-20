"""Shared Iceberg REST-catalog configuration.

All infra scripts import from this module so that catalog credentials and
connection parameters are defined in exactly one place.

Environment variables
---------------------
ICEBERG_REST_URI          REST catalog endpoint   (default ``http://localhost:8181``)
ICEBERG_S3_ENDPOINT      S3 / Garage endpoint    (default ``http://localhost:3900``)
ICEBERG_S3_ACCESS_KEY_ID S3 access key            (**required**)
ICEBERG_S3_SECRET_ACCESS_KEY  S3 secret key       (**required**)
"""

from __future__ import annotations

import os
from dataclasses import dataclass

from pyiceberg.catalog import load_catalog


@dataclass(frozen=True)
class IcebergEnv:
    """Resolved Iceberg environment from process environment variables."""

    rest_uri: str
    s3_endpoint: str
    s3_access_key: str
    s3_secret_key: str
    warehouse: str

    @classmethod
    def from_env(cls) -> IcebergEnv:
        return cls(
            rest_uri=os.environ.get("ICEBERG_REST_URI", "http://localhost:8181"),
            s3_endpoint=os.environ.get("ICEBERG_S3_ENDPOINT", "http://localhost:3900"),
            s3_access_key=os.environ["ICEBERG_S3_ACCESS_KEY_ID"],
            s3_secret_key=os.environ["ICEBERG_S3_SECRET_ACCESS_KEY"],
            warehouse="datalake",
        )


def get_catalog(warehouse: str | None = None) -> object:
    """Load the Iceberg REST catalog backed by a Garage S3 store.

    Parameters
    ----------
    warehouse:
        Warehouse identifier inside the REST catalog.  Defaults to
        ``"datalake"``.
    """
    env = IcebergEnv.from_env()
    return load_catalog(
        "iceberg",
        **{
            "type": "rest",
            "uri": env.rest_uri,
            "warehouse": warehouse or env.warehouse,
            "s3.endpoint": env.s3_endpoint,
            "s3.access-key-id": env.s3_access_key,
            "s3.secret-access-key": env.s3_secret_key,
            "s3.force-virtual-addressing": "false",
        },
    )
