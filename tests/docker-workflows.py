#!/usr/bin/env python3
"""Real container acceptance. Only uniquely named test containers/volumes are removed."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import http.client
import json
import os
from pathlib import Path
import secrets
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
import uuid


def docker(*args, check=True):
    result = subprocess.run(['docker', *map(str, args)], capture_output=True, text=True, timeout=90)
    if check and result.returncode:
        # Commands may carry credentials. Do not echo argv or arbitrary container logs.
        raise AssertionError(f'Docker operation {args[0]} failed (exit {result.returncode})')
    return result


def request(base, token, method, route, body=None, headers=None, expected=200):
    req = urllib.request.Request(base + route, data=body, method=method,
                                 headers={'Authorization': 'Bearer ' + token, **(headers or {})})
    try:
        response = urllib.request.urlopen(req, timeout=15)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        raw = response.read()
        assert response.status == expected, (method, route, response.status, expected)
        return json.loads(raw) if 'application/json' in response.headers.get('Content-Type', '') else raw


class Containers:
    def __init__(self, image):
        self.image = image
        self.prefix = 'brains-test-' + uuid.uuid4().hex[:12]
        self.names = []
        self.volumes = []

    def volume(self):
        name = self.prefix + '-data-' + str(len(self.volumes))
        docker('volume', 'create', name)
        self.volumes.append(name)
        return name

    def start(self, mount, *, env=(), extra=(), command=(), port=45800, ready=True):
        name = self.prefix + '-' + str(len(self.names))
        self.names.append(name)
        args = ['run', '-d', '--name', name, '--read-only', '--cap-drop', 'ALL',
                '--security-opt', 'no-new-privileges', '--tmpfs', '/tmp:rw,noexec,nosuid,size=64m',
                '--mount', mount, '-p', f'127.0.0.1::{port}']
        for value in env:
            args += ['-e', value]
        docker(*args, *extra, self.image, *command)
        if not ready:
            return name
        mapping = json.loads(docker('inspect', name).stdout)[0]['NetworkSettings']['Ports'][f'{port}/tcp'][0]
        base = 'http://127.0.0.1:' + mapping['HostPort']
        deadline = time.monotonic() + 40
        while time.monotonic() < deadline:
            state = json.loads(docker('inspect', name).stdout)[0]['State']
            assert state['Running'], f'container exited: {state["ExitCode"]}'
            try:
                request(base, '', 'GET', '/healthz')
                return name, base
            except OSError:
                time.sleep(0.1)
        raise AssertionError('server did not become ready')

    def stop(self, name):
        docker('stop', '--time', '15', name)
        state = json.loads(docker('inspect', name).stdout)[0]['State']
        assert state['ExitCode'] == 0 and not state['OOMKilled'], state

    def failure(self, name, message):
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            state = json.loads(docker('inspect', name).stdout)[0]['State']
            if not state['Running']:
                assert state['ExitCode'] == 2, state
                assert message in docker('logs', name).stderr
                return
            time.sleep(0.1)
        raise AssertionError('invalid configuration kept running')

    def cleanup(self):
        for name in self.names:
            docker('rm', '-f', name, check=False)
        for volume in self.volumes:
            docker('volume', 'rm', volume)


def named_volume_flow(containers):
    volume = containers.volume()
    mount = f'type=volume,src={volume},dst=/data'
    name, base = containers.start(mount, env=['BRAINS_NO_SEED=true'])
    assert docker('exec', name, 'id', '-u').stdout.strip() == '65532'
    assert docker('exec', name, 'stat', '-c', '%a', '/data/auth.json').stdout.strip() == '600'
    registry = json.loads(docker('exec', name, 'cat', '/data/auth.json').stdout)
    token = registry['tokens'][0]['plaintext']
    signing = docker('exec', name, 'sha256sum', '/data/secret.key').stdout
    request(base, 'incorrect', 'GET', '/api/v1/capabilities', expected=401)
    caps = request(base, token, 'GET', '/api/v1/capabilities')
    assert caps['runtime'] == 'local-rust' and caps['features']['fts_search']
    version = docker('run', '--rm', containers.image, '--version').stdout.strip()
    assert version == 'brains-server ' + caps['build_version']
    assert b'<html' in request(base, '', 'GET', '/')
    listing = request(base, token, 'POST', '/api/v1/files/query', b'{}', {'Content-Type': 'application/json'})
    assert not listing['items'], 'no-seed must leave an empty vault'
    route = '/api/v1/file?path=persist.md'
    first = request(base, token, 'PUT', route, b'first version',
                    {'Content-Type': 'text/markdown', 'If-None-Match': '*'})
    second = request(base, token, 'PUT', route, b'searchable second version',
                     {'Content-Type': 'text/markdown', 'If-Match': '"' + first['revision'] + '"'})
    history = request(base, token, 'GET', '/api/v1/revisions?path=persist.md')
    assert len(history['items']) == 2
    duplicate = containers.start(mount, env=['BRAINS_NO_SEED=true'], ready=False)
    containers.failure(duplicate, 'store init failed')
    containers.stop(name)
    docker('rm', name)
    name, base = containers.start(mount, env=['BRAINS_NO_SEED=true'])
    assert docker('exec', name, 'sha256sum', '/data/secret.key').stdout == signing
    request(base, token, 'GET', '/api/v1/capabilities')
    assert request(base, token, 'GET', route) == b'searchable second version'
    assert request(base, token, 'GET', '/api/v1/revision?path=persist.md&revision=' + first['revision']) == b'first version'
    assert request(base, token, 'GET', '/api/v1/revisions?path=persist.md') == history
    for mode in ('literal', 'fts'):
        result = request(base, token, 'POST', '/api/v1/search',
                         json.dumps({'q': 'searchable', 'mode': mode}).encode(), {'Content-Type': 'application/json'})
        assert [item['path'] for item in result['items']] == ['persist.md']
    # Real slow upload: wait until the handler has accepted it, signal shutdown,
    # then finish the request. Graceful shutdown must preserve the committed body.
    connection = http.client.HTTPConnection('127.0.0.1', int(base.rsplit(':', 1)[1]), timeout=15)
    body = b'completed during graceful shutdown'
    connection.putrequest('PUT', '/api/v1/file?path=shutdown.md')
    for key, value in {'Authorization': 'Bearer ' + token, 'If-None-Match': '*',
                       'Content-Type': 'text/markdown', 'Content-Length': str(len(body))}.items():
        connection.putheader(key, value)
    connection.putheader('Expect', '100-continue')
    connection.endheaders()
    interim = connection.sock.makefile('rb', buffering=0)
    assert interim.readline().startswith(b'HTTP/1.1 100')
    while interim.readline() != b'\r\n':
        pass
    with ThreadPoolExecutor(max_workers=1) as pool:
        stop = pool.submit(containers.stop, name)
        deadline = time.monotonic() + 5
        while 'shutdown requested' not in docker('logs', name).stdout:
            assert time.monotonic() < deadline, 'shutdown signal was not handled'
            time.sleep(0.05)
        connection.send(body)
        response = connection.getresponse()
        assert response.status == 200
        response.read()
        connection.close()
        interim.close()
        stop.result()
    name, base = containers.start(mount, env=['BRAINS_NO_SEED=true'])
    assert request(base, token, 'GET', '/api/v1/file?path=shutdown.md') == body
    meta = request(base, token, 'GET', route + '&format=meta')
    assert meta['revision'] == second['revision']
    containers.stop(name)
    print('PASS named volume: non-root, Web, generated auth, lock, history/FTS, rebuild, SIGTERM drains upload', flush=True)


def bind_and_secret_flow(containers, root):
    data = root / 'data'
    data.mkdir(mode=0o700)
    token = secrets.token_hex(24)
    secret = root / 'token'
    secret.write_text(token + '\n')
    secret.chmod(0o400)
    mount = f'type=bind,src={data},dst=/data'
    # On native Linux use a non-root host UID; on macOS Docker maps this bind owner.
    uid, gid = os.getuid(), os.getgid()
    if uid == 0:
        uid = gid = 12001
        os.chown(data, uid, gid)
        os.chown(secret, uid, gid)
    extra = ['--user', f'{uid}:{gid}', '--mount', f'type=bind,src={secret},dst=/run/secrets/root,readonly']
    env = ['BRAINS_TOKEN_FILE=/run/secrets/root', 'BRAINS_NO_SEED=true', 'BRAINS_PORT=45801',
           'BRAINS_VAULT=mounted', 'BRAINS_ALLOWED_ORIGINS=https://notes.example.com,https://other.example.com']
    name, base = containers.start(mount, env=env, extra=extra, port=45801)
    caps = request(base, token, 'GET', '/api/v1/capabilities')
    assert caps['vault'] == 'mounted'
    request(base, token, 'PUT', '/api/v1/file?path=bind.md', b'bind persisted',
            {'Content-Type': 'text/markdown', 'If-None-Match': '*'})
    # Exercise the actual image healthcheck command with the nondefault port.
    health = json.loads(docker('image', 'inspect', containers.image).stdout)[0]['Config']['Healthcheck']['Test']
    assert health[0] == 'CMD-SHELL'
    docker('exec', name, '/bin/sh', '-c', health[1])
    containers.stop(name)
    docker('rm', name)
    # An explicit file token persists too; the second boot needs no token input.
    name, base = containers.start(mount, env=['BRAINS_NO_SEED=true', 'BRAINS_VAULT=mounted'], extra=extra)
    assert request(base, token, 'GET', '/api/v1/file?path=bind.md') == b'bind persisted'
    containers.stop(name)
    assert (data / 'auth.json').is_file() and (data / 'secret.key').is_file()
    assert not list(data.glob('.auth-*')), 'private writes left temporary files'
    # Invalid configuration is rejected before any storage initialization.
    for i, (invalid, message) in enumerate([
        (['BRAINS_TOKEN='], 'root token must be nonempty'),
        (['BRAINS_TOKEN_FILE=/missing'], 'cannot read token file'),
        (['BRAINS_TOKEN_FILE=/run/secrets/root', 'BRAINS_TOKEN=conflicting'], 'cannot be used with'),
    ]):
        empty = root / f'invalid-{i}'
        empty.mkdir(mode=0o700)
        if os.getuid() == 0:
            os.chown(empty, uid, gid)
        failed = containers.start(f'type=bind,src={empty},dst=/data', env=invalid, extra=extra, ready=False)
        containers.failure(failed, message)
        assert not list(empty.iterdir()), 'invalid configuration initialized data'
    print('PASS bind: custom UID/GID, secret file, port/health, vault, retained token; invalid config has no data writes', flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('image')
    args = parser.parse_args()
    containers = Containers(args.image)
    try:
        named_volume_flow(containers)
        with tempfile.TemporaryDirectory(prefix=containers.prefix) as root:
            bind_and_secret_flow(containers, Path(root))
    finally:
        containers.cleanup()


if __name__ == '__main__':
    main()
