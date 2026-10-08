#!/bin/bash
set -e
B=http://127.0.0.1:8799/api/v1
T="brain-dev-token-0000000000000000"
A="Authorization: Bearer $T"
P="e2e-smoke/notes/hello-中文-$RANDOM.md"
echo "== 1. capabilities"
curl -s -H "$A" "$B/capabilities" | python3 -c "import json,sys; d=json.load(sys.stdin); print('vault:', d.get('vault'), 'text_max:', d.get('max_text_bytes'))"
echo "== 2. create -> 201"
curl -s -o /tmp/c1.json -w "put create: %{http_code}\n" -X PUT -H "$A" -H "If-None-Match: *" -H "Content-Type: text/markdown" --data-binary "第一版内容 cache-strategy" "$B/file?path=$P"
REV1=$(python3 -c "import json;print(json.load(open('/tmp/c1.json'))['revision'])")
echo "rev1=$REV1"
echo "== 3. get raw + etag"
curl -s -D /tmp/h1 -H "$A" "$B/file?path=$P" | head -1
grep -i etag /tmp/h1
echo "== 4. conditional update wrong base -> 412"
curl -s -o /tmp/c2.json -w "put stale: %{http_code}\n" -X PUT -H "$A" -H 'If-Match: "00000000000000000000000000000000"' -H "Content-Type: text/markdown" --data-binary "x" "$B/file?path=$P"
python3 -c "import json;d=json.load(open('/tmp/c2.json'));print('code:', d['error']['code'], 'current:', d['error']['current_revision'][:8])"
echo "== 5. update correct base -> 200 (gen2)"
curl -s -o /tmp/c3.json -w "put update: %{http_code}\n" -X PUT -H "$A" -H "If-Match: \"$REV1\"" -H "Content-Type: text/markdown" --data-binary "第二版内容 cache-strategy 二" "$B/file?path=$P"
REV2=$(python3 -c "import json;print(json.load(open('/tmp/c3.json'))['revision'])")
echo "rev2=$REV2"
echo "== 6. CAS RACE: two writers from rev2, exactly one 200"
(curl -s -o /tmp/raceA.json -w "%{http_code}" -X PUT -H "$A" -H "If-Match: \"$REV2\"" -H "Content-Type: text/markdown" --data-binary "writer A 独家" "$B/file?path=$P" > /tmp/raceA.code) &
(curl -s -o /tmp/raceB.json -w "%{http_code}" -X PUT -H "$A" -H "If-Match: \"$REV2\"" -H "Content-Type: text/markdown" --data-binary "writer B 独家" "$B/file?path=$P" > /tmp/raceB.code) &
wait
A_CODE=$(cat /tmp/raceA.code); B_CODE=$(cat /tmp/raceB.code)
echo "race codes: A=$A_CODE B=$B_CODE"
python3 - "$A_CODE" "$B_CODE" <<'PY'
import sys
codes = sorted(sys.argv[1:3])
assert codes == ["200", "412"], f"CAS race violated: {codes}"
print("CAS race OK: exactly one committed, one 412")
PY
echo "== 7. index_state + search literal"
sleep 0.3
curl -s -H "$A" "$B/search?q=cache-strategy&mode=literal" | python3 -c "import json,sys; d=json.load(sys.stdin); print('hits:', len(d['items']), 'complete:', d['complete'], 'state:', d['index_state'])"
echo "== 8. search FTS token"
curl -s -H "$A" "$B/search?q=strategy&mode=fts" | python3 -c "import json,sys; d=json.load(sys.stdin); print('fts hits:', len(d['items']), d['items'][0]['path'] if d['items'] else '')" || true
echo "== 9. search with prefix filter (fts must respect it)"
curl -s -H "$A" "$B/search?q=cache&mode=fts&prefix=e2e-smoke/" | python3 -c "import json,sys; d=json.load(sys.stdin); print('fts+prefix hits:', len(d['items']), [i['path'] for i in d['items']][:2])"
curl -s -H "$A" "$B/search?q=cache&mode=fts&prefix=elsewhere/" | python3 -c "import json,sys; d=json.load(sys.stdin); print('fts wrong-prefix hits:', len(d['items']))"
echo "== 10. trash -> 410 -> restore -> new gen"
HEAD_ETAG=$(curl -s -D - -o /dev/null -H "$A" "$B/file?path=$P&format=meta" | grep -i '^etag' | tr -d '\r"' | awk '{print $2}')
curl -s -o /tmp/t1.json -w "delete: %{http_code}\n" -X DELETE -H "$A" -H "If-Match: \"$HEAD_ETAG\"" "$B/file?path=$P"
curl -s -o /dev/null -w "get after trash: %{http_code}\n" -H "$A" "$B/file?path=$P"
ANCHOR=$(python3 -c "import json;print(json.load(open('/tmp/t1.json'))['revision'])")
curl -s -o /tmp/t2.json -w "restore: %{http_code}\n" -X POST -H "$A" -H "If-Match: \"$ANCHOR\"" -H "Content-Type: application/json" -d "{\"path\":\"$P\",\"revision\":\"$REV2\"}" "$B/restore"
python3 -c "import json;d=json.load(open('/tmp/t2.json'));print('restored gen:', d['generation'])"
echo "== 11. history shows restore action"
curl -s -H "$A" "$B/revisions?path=$P" | python3 -c "import json,sys; d=json.load(sys.stdin); print('actions newest-first:', [i['action'] for i in d['items']])"
echo "SMOKE CORE DONE"
