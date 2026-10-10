import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { brains as sdk } from './sdk/brains.client.ts';
import { Forbidden, Gone, NotFound, NotModified, PreconditionFailed, UnprocessableEntity } from './sdk/brains.errors.ts';

const origin = process.env.BRAINS_TEST_ORIGIN!;
const token = process.env.BRAINS_TEST_TOKEN!;
assert(origin && token, 'run with tests/http-sdk/run.sh');
const api = `${origin}/api/v1`;
const authorization = `Bearer ${token}`;
const files = new sdk.FilesClient(api, { headers: { Authorization: authorization } });
const bytes = (text: string) => new TextEncoder().encode(text);
const digest = (body: Uint8Array) => createHash('sha256').update(body).digest('hex');
const path = 'ts-sdk/a.md';
const original = bytes('# hello');

// Given an external client, byte uploads keep their media type and digest.
const created = await files.put_file(path, undefined, '*', undefined,
  digest(original), undefined, 'text/markdown', original);
assert.equal(created.return.created, true);
const revision1 = created.return.revision;
const etag1 = created.etag;
assert.equal(etag1, `"${revision1}"`);
assert(etag1);
const meta = await files.get_file(path, 'meta', undefined);
assert(meta.kind === 'Json');
assert.equal(meta.value.revision, revision1);
assert.equal(meta.value.sha256, digest(original));
assert.equal(meta.etag, etag1);
assert.equal(meta.media_type, undefined);
const raw = await files.get_file(path, undefined, undefined);
assert(raw.kind === 'OctetStream');
assert.deepEqual(raw.value, original);
assert.equal(raw.media_type, 'text/markdown');
assert.equal(raw.etag, etag1);
await assert.rejects(files.get_file(path, undefined, etag1),
  error => error instanceof NotModified && error.etag === etag1);

// When previewing a patch, the result is complete and the stored head is unchanged.
const patch = (dry_run: boolean) => ({
  edits: [{ old_text: 'hello', new_text: 'hello v2', expected_matches: 1 }], dry_run,
});
const preview = await files.patch_file(path, etag1, undefined, patch(true));
assert.equal(preview.return.base_revision, revision1);
assert.equal(preview.return.committed, undefined);
assert.equal(preview.etag, undefined);
assert.match(preview.return.diff, /\+# hello v2/);
const unchanged = await files.get_file(path, undefined, undefined);
assert(unchanged.kind === 'OctetStream');
assert.deepEqual(unchanged.value, original);
assert.equal(unchanged.etag, etag1);
const committed = await files.patch_file(path, etag1, undefined, patch(false));
assert(committed.return.committed);
const revision2 = committed.return.committed.revision;
const etag2 = committed.etag;
assert.notEqual(revision2, revision1);
assert.equal(etag2, `"${revision2}"`);
assert(etag2);
const afterCommit = await files.get_file(path, undefined, undefined);
assert(afterCommit.kind === 'OctetStream');
assert.deepEqual(afterCommit.value, bytes('# hello v2'));
assert.equal(afterCommit.etag, etag2);
await assert.rejects(files.patch_file(path, etag1, undefined, patch(false)),
  error => error instanceof PreconditionFailed && error.etag === etag2
    && error.error.current_revision === revision2);

// History, restore and trash compose with the same conditional-write contract.
const history = await files.list_revisions(path, 10, undefined);
assert.equal(history.items[0].revision, revision2);
const old = await files.get_revision(path, revision1, undefined);
assert(old.kind === 'OctetStream');
assert.deepEqual(old.value, original);
assert.equal(old.etag, etag1);
assert.equal(old.media_type, 'text/markdown');
const oldMeta = await files.get_revision(path, revision1, 'meta');
assert(oldMeta.kind === 'Json');
assert.equal(oldMeta.value.revision, revision1);
assert.equal(oldMeta.value.path, path);
assert.equal(oldMeta.value.content?.sha256, digest(original));
assert.equal(oldMeta.etag, etag1);
assert.equal(oldMeta.media_type, undefined);
const restored = await files.restore(etag2, undefined, { path, revision: revision1 });
assert.notEqual(restored.return.revision, revision1);
assert.notEqual(restored.return.revision, revision2);
const restoredRaw = await files.get_file(path, undefined, undefined);
assert(restoredRaw.kind === 'OctetStream');
assert.deepEqual(restoredRaw.value, original);
// Copy and move preserve bytes while giving each destination its own revision.
const copied = await files.copy({ source: path, target: 'ts-sdk/copied.md',
  expected_source_revision: restored.return.revision, operation_id: '11111111111111111111111111111111' });
assert(copied.copied && !copied.source_trashed && !copied.partial);
assert(copied.target_revision);
const moved = await files.move({ source: 'ts-sdk/copied.md', target: 'ts-sdk/moved.md',
  expected_source_revision: copied.target_revision, operation_id: '22222222222222222222222222222222' });
assert(moved.copied && moved.source_trashed && !moved.partial);
const movedRaw = await files.get_file('ts-sdk/moved.md', undefined, undefined);
assert(movedRaw.kind === 'OctetStream');
assert.deepEqual(movedRaw.value, original);
assert.equal(movedRaw.etag, `"${moved.target_revision}"`);
await assert.rejects(files.get_file('ts-sdk/copied.md', undefined, undefined), Gone);

// Invalid text destinations report a typed 422 before changing either path.
const binaryPath = 'ts-sdk/source.bin';
const binaryBody = new Uint8Array([0xff]);
const binary = await files.put_file(binaryPath, undefined, '*', undefined,
  digest(binaryBody), undefined, 'application/octet-stream', binaryBody);
for (const [index, transfer] of ['copy', 'move'].entries()) {
  const target = `ts-sdk/invalid-${transfer}.md`;
  const body = { source: binaryPath, target,
    expected_source_revision: binary.return.revision,
    operation_id: String(index + 3).repeat(32) };
  await assert.rejects(transfer === 'copy' ? files.copy(body) : files.move(body),
    error => error instanceof UnprocessableEntity && error.status === 422
      && error.error.code === 'INVALID_FORMAT');
  const source = await files.get_file(binaryPath, undefined, undefined);
  assert(source.kind === 'OctetStream');
  assert.deepEqual(source.value, binaryBody);
  assert.equal(source.etag, binary.etag);
  await assert.rejects(files.get_file(target, undefined, undefined), NotFound);
}
const trashed = await files.delete_file(path, restored.etag, undefined);
assert.equal(trashed.etag, `"${trashed.return.revision}"`);
await assert.rejects(files.get_file(path, undefined, undefined),
  error => error instanceof Gone && error.error.code === 'TRASHED' && error.etag === trashed.etag);

// A cookie-authenticated mutation requires Origin and the session's CSRF token.
const login = await new sdk.SessionsClient(api).login(authorization);
const cookie = login.set_cookie?.split(';')[0];
const csrf = login.return.csrf_token;
assert(cookie);
assert(cookie.startsWith('brains_session='));
assert(csrf);
assert(login.set_cookie?.includes('HttpOnly'));
const current = await new sdk.SessionsClient(api, { headers: { Cookie: cookie } }).current();
assert.equal(current.actor, login.return.actor);
assert.equal(current.csrf_token, csrf);
const noCsrfFiles = new sdk.FilesClient(api, { headers: { Cookie: cookie, Origin: origin } });
const cookieBody = bytes('cookie write');
await assert.rejects(noCsrfFiles.put_file('ts-sdk/cookie.md', undefined, '*', undefined,
  digest(cookieBody), undefined, 'text/markdown', cookieBody),
  error => error instanceof Forbidden && error.status === 403);
const cookieHeaders = { Cookie: cookie, Origin: origin, 'X-CSRF-Token': csrf };
const cookieFiles = new sdk.FilesClient(api, { headers: cookieHeaders });
const cookieCreated = await cookieFiles.put_file('ts-sdk/cookie.md', undefined, '*', undefined,
  digest(cookieBody), undefined, 'text/markdown', cookieBody);
assert.equal(cookieCreated.return.created, true);
const logout = await new sdk.SessionsClient(api, { headers: cookieHeaders }).logout();
assert.equal(logout.return.ok, true);
assert(logout.set_cookie?.includes('Max-Age=0'));
console.log('PASS generated TypeScript SDK: upload/read/preview/commit/history/copy/move/422-without-mutation/restore/trash/session/CSRF');
