#!/bin/bash
set -ex

PYTHON=${PYTHON:-python3}
ORIG_DIR=$(pwd)
STAGING_DIR=$(mktemp -d)

cleanup() {
  rm -rf "$STAGING_DIR"
}

trap cleanup EXIT

rm -rf dist
mkdir -p dist

rsync -a \
  --exclude dist \
  --exclude build \
  --exclude '.env*' \
  --exclude '*.egg-info' \
  --exclude '__pycache__' \
  --exclude '*.so' \
  --exclude '*.pyd' \
  ./ "$STAGING_DIR"/

mkdir -p "$STAGING_DIR"/sudachi-lib "$STAGING_DIR"/resources
rsync -a ../sudachi/ "$STAGING_DIR"/sudachi-lib/
rsync -a ../resources/ "$STAGING_DIR"/resources/
cp ../Cargo.lock "$STAGING_DIR"/Cargo.lock
cp ../LICENSE "$STAGING_DIR"/LICENSE

cd "$STAGING_DIR"

## Resolve workspace.package value in Cargo.toml
"$PYTHON" modify_cargotoml_for_sdist.py \
    "$ORIG_DIR"/../Cargo.toml Cargo.toml --out Cargo.toml

"$PYTHON" modify_cargotoml_for_sdist.py \
    "$ORIG_DIR"/../Cargo.toml sudachi-lib/Cargo.toml --out sudachi-lib/Cargo.toml

## Modify to include the staged path dependency and remove test-only dev dependencies.
"$PYTHON" - <<'PY'
from pathlib import Path
import tomlkit

cargo_toml = Path("Cargo.toml")
cargo_toml.write_text(
    cargo_toml.read_text(encoding="utf-8").replace("../sudachi", "./sudachi-lib"),
    encoding="utf-8",
)

sudachi_toml = Path("sudachi-lib/Cargo.toml")
parsed = tomlkit.parse(sudachi_toml.read_text(encoding="utf-8"))
parsed["package"].pop("readme", None)
parsed.pop("dev-dependencies", None)
sudachi_toml.write_text(tomlkit.dumps(parsed), encoding="utf-8")
PY


# Build the source distribution
"$PYTHON" -m build --sdist --outdir "$ORIG_DIR"/dist
