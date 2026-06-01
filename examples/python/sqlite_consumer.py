#!/usr/bin/env python3
"""Read a julie-extract SQLite artifact without Rust dependencies."""

from __future__ import annotations

import json
import sqlite3
import sys
from pathlib import Path


REQUIRED_METADATA_KEYS = [
    "schema_version",
    "extract_contract_version",
    "sqlite_schema_version",
    "root_path",
]

TABLES = [
    "files",
    "symbols",
    "identifiers",
    "relationships",
    "pending_relationships",
]


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print("usage: sqlite_consumer.py <artifact.sqlite>", file=sys.stderr)
        return 2

    try:
        summary = read_summary(Path(argv[1]))
    except ConsumerError as error:
        print(error, file=sys.stderr)
        return 1

    json.dump(summary, sys.stdout, sort_keys=True)
    sys.stdout.write("\n")
    return 0


def read_summary(path: Path) -> dict[str, object]:
    if not path.is_file():
        raise ConsumerError(f"artifact does not exist: {path}")

    uri = path.resolve().as_uri() + "?mode=ro"
    try:
        with sqlite3.connect(uri, uri=True) as connection:
            metadata = read_metadata(connection)
            tables = {table: table_count(connection, table) for table in TABLES}
    except sqlite3.Error as error:
        raise ConsumerError(f"failed to read artifact: {error}") from error

    for key in REQUIRED_METADATA_KEYS:
        if key not in metadata:
            raise ConsumerError(f"missing metadata `{key}`")

    if tables["files"] == 0:
        raise ConsumerError("table `files` has zero rows")

    return {
        "artifact": str(path),
        "root_path": metadata["root_path"],
        "schema_version": int_metadata(metadata, "schema_version"),
        "extract_contract_version": int_metadata(metadata, "extract_contract_version"),
        "sqlite_schema_version": int_metadata(metadata, "sqlite_schema_version"),
        "tables": tables,
    }


def read_metadata(connection: sqlite3.Connection) -> dict[str, str]:
    rows = connection.execute("SELECT key, value FROM artifact_metadata").fetchall()
    return {key: value for key, value in rows}


def table_count(connection: sqlite3.Connection, table: str) -> int:
    row = connection.execute(f"SELECT COUNT(*) FROM {table}").fetchone()
    if row is None:
        raise ConsumerError(f"table `{table}` did not return a row count")
    return int(row[0])


def int_metadata(metadata: dict[str, str], key: str) -> int:
    try:
        return int(metadata[key])
    except ValueError as error:
        raise ConsumerError(f"metadata `{key}` is not an integer") from error


class ConsumerError(Exception):
    pass


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
