"""Narrow xidlc 0.95 wire corrections. Delete when upstream handles raw bodies.

IDL owns parameter names and flatten semantics; this postprocessor corrects
raw octets and the TypeScript flattened-body template with exact assertions.
"""
import json
from pathlib import Path


def replace_once(text, old, new):
    if text.count(old) != 1:
        raise SystemExit(f"unexpected generator template: {old[:80]!r}")
    return text.replace(old, new)


def raw_rust(text, name, content_type):
    start = text.index(f"pub async fn {name}(")
    end = text.find("pub async fn ", start + 1)
    end = len(text) if end < 0 else end
    method = text[start:end]
    first = method.index('            let body: Vec<u8> = payload.into_iter().collect();')
    last = method.index('            *req.body_mut()', first)
    header = '' if content_type else '            req.headers_mut().insert(xidl_rust_axum::reqwest::header::CONTENT_TYPE, xidl_rust_axum::reqwest::header::HeaderValue::from_static("application/octet-stream"));\n'
    method = method[:first] + '            let body = payload;\n' + header + method[last:]
    return text[:start] + method + text[end:]

rust = Path('/tmp/brains-sdk-rs/brains.rs')
text = rust.read_text()
text = raw_rust(text, 'put_file', True)
text = raw_rust(text, 'import_archive_blob', False)
rust.write_text(text)

ts = Path('sdk/typescript/brains.client.ts')
text = ts.read_text()
# All eight payload methods declare exactly one flattened body in the IDL.
if text.count('const body = parsed;') != 8:
    raise SystemExit('flattened TypeScript payload template changed')
text = text.replace('const body = parsed;', 'const body = parsed.payload;')
for name, content_type in [('put_file', 'content_type'), ('import_archive_blob', '"application/octet-stream"')]:
    start = text.index(f'async {name}(')
    end = text.find('async ', start + 1)
    method = text[start:end]
    method = replace_once(method, 'headers.set("Content-Type", "application/json");', f'headers.set("Content-Type", {content_type});')
    method = replace_once(method, 'encodeRequestBody(body, "application/json", this.codecs, undefined)', 'new Uint8Array(body)')
    text = text[:start] + method + text[end:]
ts.write_text(text)

path = Path('sdk/openapi/brains-openapi.json')
spec = json.loads(path.read_text())
for route in ['/file', '/maintenance/archive/blob']:
    op = spec['paths'][route]['put']
    op['requestBody'] = {'required': True, 'content': {'application/octet-stream': {'schema': {'type': 'string', 'format': 'binary'}}}}
# Raw Revision uses explicit null for parent/content; optional != nullable.
revision_key = next(key for key in spec['components']['schemas'] if key.endswith('ArchiveRevision'))
for name in ['parent', 'content']:
    props = spec['components']['schemas'][revision_key]['properties']
    props[name] = {'anyOf': [props[name], {'type': 'null'}]}
path.write_text(json.dumps(spec, ensure_ascii=False, indent=2) + '\n')

zod = Path('sdk/typescript/brains.zod.ts')
text = zod.read_text()
start = text.index('export const ArchiveRevisionSchema')
end = text.index('});', start)
block = text[start:end]
block = replace_once(block, 'parent: z.coerce.string().optional()', 'parent: z.string().nullable().optional()')
block = replace_once(block, 'content: ArchiveContentSchema.optional()', 'content: ArchiveContentSchema.nullable().optional()')
zod.write_text(text[:start] + block + text[end:])

types = Path('sdk/typescript/brains.d.ts')
text = types.read_text()
start = text.index('export interface ArchiveRevision')
end = text.index('}', start)
block = replace_once(text[start:end], 'parent?: string;', 'parent?: string | null;')
block = replace_once(block, 'content?: ArchiveContent;', 'content?: ArchiveContent | null;')
types.write_text(text[:start] + block + text[end:])
