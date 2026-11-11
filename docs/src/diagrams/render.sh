#!/usr/bin/env bash
# Re-render the book's diagrams after editing a .dot file: docs/src/diagrams/render.sh
# Needs Graphviz (`dot`). The SVGs are committed, so building the book does not.
set -euo pipefail
cd "$(dirname "$0")"
for f in *.dot; do
    dot -Tsvg "$f" -o "${f%.dot}.svg"
    echo "rendered ${f%.dot}.svg"
done
