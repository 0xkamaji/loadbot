#!/usr/bin/env sh

set -eu

ARCHIVE_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
exec "$ARCHIVE_DIR/loadbot" setup --all
