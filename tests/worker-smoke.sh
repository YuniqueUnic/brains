#!/bin/sh
set -eu
# The isolated runner supplies these; direct use against an explicit instance is also supported.
: "${BRAINS_BASE_URL:?run through scripts/run-workerd.py or set BRAINS_BASE_URL}"
: "${BRAINS_E2E_TOKEN:?run through scripts/run-workerd.py or set BRAINS_E2E_TOKEN}"
exec python3 - <<'PY'
from concurrent.futures import ThreadPoolExecutor
import json
import os
import threading
import urllib.error
import urllib.parse
import urllib.request
import uuid

base = os.environ['BRAINS_BASE_URL'].rstrip('/') + '/api/v1'
token = os.environ['BRAINS_E2E_TOKEN']
path = f'e2e-smoke/notes/hello-中文-{uuid.uuid4().hex}.md'
file_route = '/file?' + urllib.parse.urlencode({'path': path})


def request(method, route, body=None, headers=None, expected=200):
    req = urllib.request.Request(base + route, data=body, method=method,
                                 headers={'Authorization': 'Bearer ' + token, **(headers or {})})
    try:
        response = urllib.request.urlopen(req, timeout=30)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        raw = response.read()
        assert response.status == expected, f'{method} {route}: expected {expected}, got {response.status}: {raw!r}'
        value = json.loads(raw) if 'application/json' in response.headers.get('Content-Type', '') else raw
        return value, response.headers


def put(content, revision=None, expected=200):
    headers = {'Content-Type': 'text/markdown'}
    headers['If-Match' if revision else 'If-None-Match'] = f'"{revision}"' if revision else '*'
    result, response_headers = request('PUT', file_route, content.encode(), headers, expected)
    if expected == 200:
        assert result['created'] is (revision is None), result
        assert result['path'] == path, result
        assert response_headers['ETag'] == f'"{result["revision"]}"'
    return result


caps, _ = request('GET', '/capabilities')
assert caps['runtime'] == 'cloudflare-rust' and caps['limits']['max_text_bytes'] > 0
print('capabilities: cloudflare-rust with text limits', flush=True)
first_text = '第一版内容 cache-strategy'
first = put(first_text)
assert first['generation'] == 1
content, headers = request('GET', file_route)
assert content == first_text.encode() and headers['ETag'] == f'"{first["revision"]}"'
stale = put('stale', '0' * 32, expected=412)
assert stale['error']['current_revision'] == first['revision']
second_text = '第二版内容 cache-strategy 二'
second = put(second_text, first['revision'])
assert second['generation'] == 2
print('create, bytes/ETag, stale rejection, update: passed', flush=True)

barrier = threading.Barrier(2)

def race_writer(name):
    barrier.wait()
    req = urllib.request.Request(base + file_route, data=f'writer {name} 独家 cache-strategy'.encode(), method='PUT',
        headers={'Authorization': 'Bearer ' + token, 'Content-Type': 'text/markdown', 'If-Match': f'"{second["revision"]}"'})
    try:
        response = urllib.request.urlopen(req, timeout=30)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        response.read()
        return response.status

with ThreadPoolExecutor(max_workers=2) as pool:
    statuses = sorted(pool.map(race_writer, ['A', 'B']))
assert statuses == [200, 412], statuses
print('CAS race: exactly one commit and one conflict', flush=True)

for query, mode, prefix, expected_hit in [
    ('cache-strategy', 'literal', None, True),
    ('strategy', 'fts', None, True),
    ('cache', 'fts', 'e2e-smoke/', True),
    ('cache', 'fts', 'elsewhere/', False),
]:
    params = {'q': query, 'mode': mode}
    if prefix: params['prefix'] = prefix
    result, _ = request('POST', '/search', json.dumps(params).encode(), {'Content-Type': 'application/json'})
    assert result['complete'] and result['index_state'] == 'available', result
    assert (path in [item['path'] for item in result['items']]) == expected_hit, result
    if not expected_hit: assert result['items'] == [], result
print('literal, FTS, included/excluded prefix: passed', flush=True)

head, _ = request('GET', file_route + '&format=meta')
assert head['generation'] == 3
trashed, _ = request('DELETE', file_route, headers={'If-Match': f'"{head["revision"]}"'})
assert trashed['generation'] == 4
request('GET', file_route, expected=410)
restored, _ = request('POST', '/restore', json.dumps({'path': path, 'revision': second['revision']}).encode(),
    {'Content-Type': 'application/json', 'If-Match': f'"{trashed["revision"]}"'})
assert restored['generation'] == 5
assert request('GET', file_route)[0] == second_text.encode()
history, _ = request('GET', '/revisions?' + urllib.parse.urlencode({'path': path}))
assert [item['action'] for item in history['items']] == ['restore', 'trash', 'put', 'put', 'put'], history
assert [item['generation'] for item in history['items']] == [5, 4, 3, 2, 1], history
print('trash, 410, restore bytes, complete history: passed', flush=True)
print('SMOKE CORE PASSED', flush=True)
PY
