#!/usr/bin/env python3
"""Check generated local HTML links, unique IDs and search result addresses."""
import argparse
from html.parser import HTMLParser
import json
from pathlib import Path
from urllib.parse import unquote, urlsplit


class Document(HTMLParser):
    def __init__(self, path):
        super().__init__()
        self.ids = set()
        self.links = []
        self.path = path

    def handle_starttag(self, tag, attributes):
        values = dict(attributes)
        if "id" in values:
            identifier = values["id"]
            if identifier in self.ids:
                raise ValueError(f"{self.path}: duplicate id {identifier!r}")
            self.ids.add(identifier)
        for attribute in ("href", "src", "action"):
            if attribute in values:
                self.links.append(values[attribute])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("site", type=Path)
    root = parser.parse_args().site.resolve(strict=True)
    documents = {}
    for path in sorted(root.rglob("*.html")):
        document = Document(path.relative_to(root))
        document.feed(path.read_text(encoding="utf-8"))
        documents[path.resolve()] = document
    if root / "index.html" not in documents:
        raise ValueError("missing index.html")

    def check(source, value):
        url = urlsplit(value)
        if url.scheme in ("http", "https", "mailto"):
            return
        if url.scheme or url.netloc or url.path.startswith("/"):
            raise ValueError(f"{source}: unsupported URL {value!r}")
        target = (source.parent / unquote(url.path)).resolve() if url.path else source
        if root != target and root not in target.parents:
            raise ValueError(f"{source}: URL escapes generated site: {value!r}")
        if not target.is_file():
            raise ValueError(f"{source}: missing link target {value!r}")
        if url.fragment:
            document = documents.get(target)
            if document is None or unquote(url.fragment) not in document.ids:
                raise ValueError(f"{source}: missing fragment {value!r}")

    count = 0
    for source, document in documents.items():
        for value in document.links:
            check(source, value)
            count += 1
    data = (root / "assets/search-index.js").read_text(encoding="utf-8").strip()
    prefix = "window.RILL_DOCS_INDEX="
    if not data.startswith(prefix) or not data.endswith(";"):
        raise ValueError("unsupported search index format")
    entries = json.loads(data[len(prefix):-1])
    for entry in entries:
        check(root / "search.html", entry["url"])
    print(f"Docs links passed: {len(documents)} HTML pages, {count} links, {len(entries)} search addresses.")


if __name__ == "__main__":
    main()
