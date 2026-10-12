"""Shared name helpers."""
import re

LATIN = re.compile(r"^[\x00-ɏḀ-ỿ]+$")


def english_name(tags):
    """name:en, then int_name, then a Latin-script name:*; else None (docs/01-features.md)."""
    if tags.get("name:en"):
        return tags["name:en"]
    if tags.get("int_name"):
        return tags["int_name"]
    for k, v in tags.items():
        if k.startswith("name:") and v and LATIN.match(v):
            return v
    return None
