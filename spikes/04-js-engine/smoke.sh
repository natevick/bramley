#!/usr/bin/env bash
set -uo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
MK="$HERE/../02-musickit-surface/vendor-cache/musickit.js"
for eng in quickjs v8; do
  for mode in logged plain; do
    out="$HERE/results/smoke-$eng-$mode.log"
    timeout 60 "$HERE/$eng/target/release/spike-$eng" --musickit "$MK" --hold 2 --max-wait 10 --mode "$mode" > "$out" 2>&1
    echo "== $eng/$mode exit=$?"
    grep -E '^(MK|TIMING|POINT|FETCH|SCRIPT|NOPOINT|APPERR|REPORTERR)' "$out" | cut -c1-400
    grep '^REPORT ' "$out" | python3 -I -c "import json,sys; r=json.loads(sys.stdin.read()[7:]); print('configure:',r.get('configure'),'| apiProbe:',r.get('apiProbe'),'| inst:',r.get('instState'),'| errors:',r.get('errors')[:3],'| console:',r.get('consoleLines')[:3],'| touched:',len(r.get('touched')),'| engineOwn:',r.get('engineOwnHostGlobals'))"
  done
done
