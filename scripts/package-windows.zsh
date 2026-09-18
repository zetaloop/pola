#!/usr/bin/env zsh
set -e

cargo build --release
print target/release
