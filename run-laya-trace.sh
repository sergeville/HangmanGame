#!/bin/sh
set -eu

cd "$(dirname "$0")"
export HANGMAN_TRACE=1
export HANGMAN_LOCAL_MODEL_PORT=8010
export HANGMAN_LOCAL_MODEL_NAME=english
exec cargo run --release --locked "$@"
