#!/usr/bin/env python3
"""Exercise separately built native FTS/basic binaries against the same vault."""
import argparse
import json
from pathlib import Path
import tempfile

from cli_support import Cli, exercise_path_subtrees, native_server


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('fts-cli', 'basic-cli', 'fts-server', 'basic-server'):
        parser.add_argument('--' + name, type=Path, required=True)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix='brains-build-features-') as directory:
        root = Path(directory)
        full = Cli(root, args.fts_cli.resolve())
        basic = Cli(root, args.basic_cli.resolve())
        data = root / 'data'
        full('local', 'init', data)
        full('local', 'put', 'note.md', '--create', data=b'oldword\n')
        old = full.stat('local', 'note.md')['revision']
        full('local', 'put', 'discard.md', '--create', data=b'discardword\n')
        snapshots = {}
        for name, cli, available in [('basic', basic, False), ('fts', full, True)]:
            caps = json.loads(cli('local', 'capabilities', '--json'))['features']
            snapshots[name] = {key: caps[key] for key in ('fts_search', 'fts_tantivy')}
            assert caps['fts_search'] == caps['fts_tantivy'] == available
            if name == 'basic':
                basic.put('local', 'note.md', b'---\ntags: [build, build]\n---\nnewword [A](target.md) [B](target.md)\n')
                current = basic.stat('local', 'note.md')['revision']
                basic('local', 'put', 'created.md', '--create', data=b'createdword\n')
                basic('local', 'rm', 'discard.md', '--if-revision', basic.stat('local', 'discard.md')['revision'])
                basic('local', 'search', 'newword', '--mode', 'fts', expected=9)
            else:
                assert full.stat('local', 'note.md')['revision'] == current
                assert b'created.md' in full('local', 'search', 'createdword', '--mode', 'fts')
                full('local', 'search', 'discardword', '--mode', 'fts', expected=1)
                assert b'note.md' in full('local', 'search', 'newword', '--mode', 'fts', '--tag', 'build')
                assert full('local', 'search', 'oldword', '--mode', 'fts', '--json', expected=1) == b''
            assert b'note.md' in cli('local', 'search', 'newword', '--mode', 'literal')
            assert len(cli('local', 'links', 'note.md', '--json').splitlines()) == 1
            assert cli('local', 'show', 'note.md', '--revision', old) == b'oldword\n'
        expected = json.loads((Path(__file__).parent / 'snapshots/native-features.json').read_text())
        assert snapshots == expected
        assert (data / 'vaults/default/index.turso-fts.db').is_file()
        assert not (data / 'vaults/default/index.turso-basic.db').exists()
        assert not list((data / 'vaults/default').glob('index.turso-building.db*'))
        print('build switch: FTS → basic edit → FTS rebuild; history, tags, links, fresh search and capability snapshot passed')
        for binary, available in [(args.basic_server, False), (args.fts_server, True)]:
            with native_server(root, data, 'default', binary.resolve()) as (url, token):
                name = 'remote-fts' if available else 'remote-basic'
                basic.remote(name, url, token)
                caps = json.loads(basic(name, 'capabilities', '--json'))['features']
                assert caps['fts_search'] == available
                # The server controls remote capabilities, independently of this CLI's build.
                basic(name, 'search', 'newword', '--mode', 'fts', expected=0 if available else 9)
                assert basic.stat(name, 'note.md')['revision'] == current
        print('remote: basic CLI uses FTS server; FTS-disabled server explicitly rejects full-text requests')
        basic('local', 'export', root / 'archive', '--format', 'archive')
        basic('copy', 'init', root / 'copy')
        basic('copy', 'import', root / 'archive', '--format', 'archive')
        assert basic('local', 'history', 'note.md', '--json') == full('copy', 'history', 'note.md', '--json')
        assert b'note.md' in full('copy', 'search', 'newword', '--mode', 'fts')
        print('archive: basic build exports/restores complete history; FTS build rebuilds searchable projection')
        exercise_path_subtrees(full, 'copy')
        print('path subtrees: literal/FTS/store/index Unicode and sibling boundaries passed')


if __name__ == '__main__':
    main()
