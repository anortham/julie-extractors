"""Tier 1 (same-file) for Python: the intra-file call ``alpha()`` is an
extraction-time relationship propagated onto the co-located identifier."""


def alpha():
    pass


def helper():
    alpha()
