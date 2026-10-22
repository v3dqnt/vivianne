#!/bin/sh
# Rebuild every captured scene listed in scenes.txt (template:output:states).
# app.css is the union of all states, so they are built together.
cd "$(dirname "$0")" && python build_app.py ../.. ../../../gui-next $(cat scenes.txt)
