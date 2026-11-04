#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
venv="${KK_X8_VENV:-$HOME/.venvs/docling-x8}"
cache="${KK_X8_CACHE:-$HOME/.cache/kk-x8}"
source "$venv/bin/activate"
mkdir -p "$cache"
[ -d "$cache/models" ] || docling-tools models download -o "$cache/models"
rm -rf "$cache/docs"
python "$here/make_docs.py" "$cache/docs"
export HF_HUB_OFFLINE=1 TRANSFORMERS_OFFLINE=1
sandbox-exec -f "$here/deny_net.sb" python "$here/parse_offline.py" "$cache/docs" "$cache/models" > "$cache/out.json"
python "$here/score.py" "$cache/docs/truth.json" "$cache/out.json"
python -c "import json,sys; a=json.load(open('$cache/out.json'))['network_attempts']; print('network attempts:', a); sys.exit(1 if a else 0)"
