// Local-only measurement fixture. Never deploy this module. Every measured
// result comes from the real workerd D1 binding; AI Search has no network I/O.
import application from "../../apps/worker/build/worker/shim.mjs";

let measurements = [];
let nextBatch = 0;
let failReconcile = false;
let control = {};
let faultCount = 0;
let held = false;
let savedProfile;
const originals = new WeakMap();

async function measured(result, sql, operation, batch) {
  sql = sql.replace(/\s+/g, " ").trim();
  measurements.push({
    sql,
    operation,
    batch,
    rows_read: result.meta?.rows_read,
    rows_written: result.meta?.rows_written,
  });
  if (control.holdSQL && sql.includes(control.holdSQL) && !held) {
    held = true;
    await new Promise(resolve => setTimeout(resolve, 2000));
    held = false;
  }
  if (control.fault?.sql && sql.includes(control.fault.sql)) {
    faultCount++;
    if (faultCount === (control.fault.at ?? 1)) {
      result = { ...result, meta: { ...result.meta } };
      if (control.fault.missing === "meta") delete result.meta;
      else delete result.meta[control.fault.missing];
    }
  }
  return result;
}

function statement(native, sql) {
  const wrapped = new Proxy(native, {
    get(target, property) {
      if (property === "constructor") return target.constructor;
      if (property === "bind") return (...values) => statement(target.bind(...values), sql);
      if (property === "all" || property === "run") {
        return async (...args) => {
          if (failReconcile && sql.includes("FROM semantic_items")) {
            throw new Error("simulated local reconciliation query failure");
          }
          return measured(await target[property](...args), sql, property);
        };
      }
      if (property === "first" || property === "raw") {
        throw new Error("The ledger fixture requires result metadata for every statement");
      }
      const value = Reflect.get(target, property);
      return typeof value === "function" ? value.bind(target) : value;
    },
  });
  originals.set(wrapped, { native, sql });
  return wrapped;
}

function database(native) {
  return new Proxy(native, {
    get(target, property) {
      if (property === "constructor") return target.constructor;
      if (property === "prepare") return sql => statement(target.prepare(sql), sql);
      if (property === "batch") return async statements => {
        const source = statements.map(value => originals.get(value));
        if (source.some(value => !value)) throw new Error("Unmeasured batch statement");
        const batch = nextBatch++;
        if (control.failClosing && source.some(value => value.sql.replace(/\s+/g, " ").trim().startsWith("UPDATE resource_usage SET consumed=consumed+"))) {
          control.failClosing = false;
          throw new Error("simulated lost closing connection before commit");
        }
        const results = await target.batch(source.map(value => value.native));
        return Promise.all(results.map((result, index) => measured(result, source[index].sql, "batch", batch)));
      };
      if (property === "exec" || property === "withSession") {
        throw new Error("Unexpected D1 execution path in the ledger fixture");
      }
      const value = Reflect.get(target, property);
      return typeof value === "function" ? value.bind(target) : value;
    },
  });
}

function testEnv(original) {
  return {
    ...original,
    BRAINS_INDEX: database(original.BRAINS_INDEX),
    SEMANTIC_SEARCH: "on",
    BRAINS_CLOUD_CONFIG: JSON.stringify({ semantic: {
      instance: "local-ledger-fixture", publication: "manual",
    } }),
    AI_SEARCH: { get: () => ({ async search() { return { chunks: [] }; } }) },
    ...control.overrides,
  };
}

async function isAdmin(request, env) {
  const token = request.headers.get("Authorization")?.replace(/^Bearer /, "") ?? "";
  const bytes = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(token));
  const hash = [...new Uint8Array(bytes)].map(byte => byte.toString(16).padStart(2, "0")).join("");
  return JSON.parse(env.TOKEN_REGISTRY_JSON).tokens.some(value =>
    value.enabled && value.token_sha256 === hash && value.scopes.includes("admin:/"));
}

async function usage(native) {
  // Measurement probes are deliberately outside the measured request workload.
  const result = await native.prepare(
    "SELECT rule,SUM(consumed) AS consumed,SUM(reserved) AS reserved FROM resource_usage GROUP BY rule ORDER BY rule",
  ).all();
  return result.results;
}

export default {
  async fetch(request, original, ctx) {
    const path = new URL(request.url).pathname;
    if (path.startsWith("/_ledger/")) {
      if (!await isAdmin(request, original)) return new Response("unauthorized", { status: 401 });
      if (path === "/_ledger/control") {
        if (request.method === "POST") {
          control = await request.json(); faultCount = 0; held = false;
        }
        return Response.json({ held, faultCount });
      }
      if (path === "/_ledger/profile") {
        const { invalid } = await request.json();
        if (invalid) {
          savedProfile = await original.BRAINS_INDEX.prepare("SELECT profile FROM resource_settings WHERE id=1").first("profile");
        }
        await original.BRAINS_INDEX.prepare("UPDATE resource_settings SET profile=?1 WHERE id=1")
          .bind(invalid ? "{" : savedProfile).run();
        return Response.json({ invalid });
      }
      if (path === "/_ledger/probe-closing") {
        const { sql, charges, seedSql, seed = [], held = false } = await request.json();
        if (seed.length) {
          await original.BRAINS_INDEX.batch(seed.map(charge =>
            original.BRAINS_INDEX.prepare(seedSql).bind(JSON.stringify(charge))));
          if (held) {
            await original.BRAINS_INDEX.batch(seed.map(charge => original.BRAINS_INDEX.prepare(
              "UPDATE resource_usage SET reserved=3 WHERE key=?1",
            ).bind(charge.key)));
          }
        }
        const execute = () => original.BRAINS_INDEX.batch(charges.map(charge =>
          original.BRAINS_INDEX.prepare(sql).bind(JSON.stringify(charge))));
        const fresh = await execute();
        const existing = await execute();
        const costs = results => ({
          rows_read: results.reduce((sum, result) => sum + result.meta.rows_read, 0),
          rows_written: results.reduce((sum, result) => sum + result.meta.rows_written, 0),
        });
        const reserved = await original.BRAINS_INDEX.prepare(
          "SELECT SUM(reserved) AS held FROM resource_usage WHERE key IN (SELECT json_extract(value,'$.key') FROM json_each(?1))",
        ).bind(JSON.stringify(charges)).first("held");
        return Response.json({ fresh: costs(fresh), existing: costs(existing), reserved });
      }
      if (path === "/_ledger/tick") {
        failReconcile = new URL(request.url).searchParams.has("fail");
        try {
          await new application(ctx, testEnv(original)).scheduled({ scheduledTime: Date.now(), cron: "fixture" });
        } finally { failReconcile = false; }
      }
      const rows = await usage(original.BRAINS_INDEX);
      if (path === "/_ledger/start") { measurements = []; nextBatch = 0; }
      return Response.json({ usage: rows, measurements });
    }
    const env = testEnv(original);
    const background = [];
    const instance = new application({
      waitUntil(work) { background.push(work); ctx.waitUntil(work); },
    }, env);
    const response = await instance.fetch(request);
    await Promise.all(background);
    if (response.status >= 500) console.error("Local ledger fixture error:", await response.clone().text());
    return response;
  },
};
