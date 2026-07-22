#!/usr/bin/env sh
set -eu

cargo test -p crownline_server --release scheduled_server_soak -- --ignored --nocapture --test-threads=1
