#!/usr/bin/env sh
set -eu

cargo test -p crownline --release scheduled_performance_baseline -- --ignored --nocapture --test-threads=1
