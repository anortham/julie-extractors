# Python SQLite Consumer

This example reads a `julie-extract` SQLite artifact with only the Python
standard library.

Run it against the dogfood artifact:

```bash
python3 examples/python/sqlite_consumer.py target/dogfood/julie-extractors/artifact.sqlite
```

The script opens the database read-only, checks required metadata, counts key
tables, and prints a compact JSON summary. It exits nonzero if required metadata
is missing or the artifact has zero file rows.
