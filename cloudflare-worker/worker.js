const LS_BASE = 'https://api.lemonsqueezy.com/v1/licenses';
// Main API (not the License API) — used to look up a key's existing activations
// so a stale one from the same machine can be reclaimed. Requires an API key
// with access to the license-key-instances endpoint.
const LS_INSTANCES = 'https://api.lemonsqueezy.com/v1/license-key-instances';
// Secrets and bindings arrive on `env` per invocation (ES-module format — D1
// bindings are not available to the old service-worker format). bindEnv copies
// them here so the handlers below can keep using plain names.
let LS_API_KEY;
let RESEND_API_KEY;
let ANALYTICS;

function bindEnv(env) {
  LS_API_KEY = env.LS_API_KEY;
  RESEND_API_KEY = env.RESEND_API_KEY;
  ANALYTICS = env.ANALYTICS;
}
const FEEDBACK_TO = 'tonictech.inquiry@gmail.com';

const CORS_HEADERS = {
  'Access-Control-Allow-Origin': '*',
  'Access-Control-Allow-Methods': 'GET, POST, OPTIONS',
  'Access-Control-Allow-Headers': 'Content-Type',
};

export default {
  async fetch(request, env) {
    bindEnv(env);
    return handleRequest(request);
  },
  // Weekly report. Cron is configured in wrangler.toml.
  async scheduled(event, env, ctx) {
    bindEnv(env);
    ctx.waitUntil(sendWeeklyReport());
  },
};

async function handleRequest(request) {
  // Handle CORS preflight
  if (request.method === 'OPTIONS') {
    return new Response(null, { status: 204, headers: CORS_HEADERS });
  }

  const url = new URL(request.url);
  const action = url.pathname.slice(1);

  if (action === 'downloads' && request.method === 'GET') {
    return handleDownloads();
  }

  if (request.method !== 'POST') {
    return new Response('Method not allowed', { status: 405, headers: CORS_HEADERS });
  }

  if (!['activate', 'deactivate', 'validate', 'feedback', 'ping'].includes(action)) {
    return new Response('Not found', { status: 404, headers: CORS_HEADERS });
  }

  let body;
  try {
    body = await request.json();
  } catch {
    return json({ error: 'Invalid JSON body' }, 400);
  }

  // Anonymous weekly usage ping. Does not touch LemonSqueezy.
  if (action === 'ping') {
    return handlePing(body);
  }

  // Handle feedback separately — does not touch LemonSqueezy
  if (action === 'feedback') {
    // `machine` matches the instance_name shown against the customer's
    // activation in Lemon Squeezy ("PCNAME [fingerprint]"), so a message can be
    // traced back to an order. Older clients don't send it.
    const machine = typeof body.machine === 'string' ? body.machine : 'unknown';
    const version = typeof body.version === 'string' ? body.version : 'unknown';

    const hasConfig = typeof body.config === 'string' && body.config.length > 0;
    const attachmentName = `config-${safeFilePart(machine)}.json`;
    const replyEmail = typeof body.reply_email === 'string' ? body.reply_email.trim() : '';

    // Header first, message second. The identifying details are the thing you
    // need before deciding whether to read on, and burying them under a long
    // message means scrolling to find them.
    const text = [
      `Machine:  ${machine}`,
      `Version:  ${version}`,
      `Reply to: ${replyEmail || 'not provided'}`,
      `Config:   ${hasConfig ? `attached as ${attachmentName}` : 'not included'}`,
      '',
      '────────────────────────────',
      '',
      body.message || '(no message)',
    ].join('\n');

    const payload = {
      from: 'Tonic Tech Feedback <onboarding@resend.dev>',
      to: FEEDBACK_TO,
      subject: `Tonic Tech Feedback — ${machine}`,
      text,
    };

    // Set Reply-To only when the address is plausible. Resend rejects the
    // entire request on a malformed reply_to, and losing the whole message
    // because someone fat-fingered their address would be the worst outcome —
    // the raw text is in the header above either way.
    if (looksLikeEmail(replyEmail)) {
      payload.reply_to = replyEmail;
    }

    // Attached rather than inlined so it can be saved straight to disk and fed
    // back through the app's own config import.
    if (hasConfig) {
      payload.attachments = [{
        filename: attachmentName,
        content: base64Utf8(body.config),
      }];
    }

    const res = await fetch('https://api.resend.com/emails', {
      method: 'POST',
      headers: {
        'Authorization': `Bearer ${RESEND_API_KEY}`,
        'Content-Type': 'application/json',
      },
      body: JSON.stringify(payload),
    });
    return res.ok ? json({ ok: true }) : json({ error: 'Failed to send feedback' }, 500);
  }

  // LemonSqueezy license actions
  const lsRes = await fetch(`${LS_BASE}/${action}`, {
    method: 'POST',
    headers: {
      'Authorization': `Bearer ${LS_API_KEY}`,
      'Content-Type': 'application/json',
      'Accept': 'application/json',
    },
    body: JSON.stringify(body),
  });

  const data = await lsRes.json();

  if (action === 'activate') {
    if (lsRes.ok && data.activated) {
      return json({ instance_id: data.instance.id, instance_name: data.instance.name });
    }
    // Activation failed. If it failed *only* because the key is out of
    // activations, and one of those activations belongs to this same machine,
    // reclaim it and try once more. Returns null if it can't safely help, in
    // which case the original error is surfaced unchanged.
    const healed = await reclaimAndRetryActivate(body, data);
    if (healed) return healed;

    return json({ error: data.error || data.errors?.[0]?.detail || 'Activation failed' }, 400);
  }

  if (action === 'deactivate') {
    if (lsRes.ok && data.deactivated) {
      return json({ ok: true });
    }
    return json({ error: data.error || 'Deactivation failed' }, 400);
  }

  // validate
  return json({ valid: lsRes.ok && data.valid === true });
}

// Self-healing reactivation.
//
// Every /activate call creates a NEW instance in Lemon Squeezy — nothing
// deduplicates by machine. So a customer who reinstalls Windows, replaces
// config.json, or otherwise loses their saved instance_id permanently burns an
// activation: the app can no longer call /deactivate for it because it no
// longer knows the id. Eventually they hit the limit and are locked out with no
// way to recover except contacting support.
//
// This reclaims that specific situation: when activation fails *because the key
// is out of activations*, look at the key's existing instances and delete any
// registered under the SAME instance_name (the machine name) before retrying
// once. Someone genuinely trying to use one key on a second machine still gets
// blocked, because that machine's name won't match.
//
// Deliberately conservative — every failure path returns null so the caller
// surfaces Lemon Squeezy's original error. This must never turn a clear error
// into a confusing one, and must never widen access beyond the same machine.
// If LS_API_KEY lacks access to the license-key-instances endpoint, the list
// call simply fails and behaviour falls back to exactly what it is today.
async function reclaimAndRetryActivate(body, failedData) {
  try {
    const licenseKey = body?.license_key;
    const wantedName = body?.instance_name;
    const licenseKeyId = failedData?.license_key?.id;
    const errMsg = String(failedData?.error || '');

    if (!licenseKey || !wantedName || !licenseKeyId) return null;
    // Only ever act on the activation-limit error. An invalid, expired or
    // disabled key must keep reporting its real problem.
    if (!/activation limit/i.test(errMsg)) return null;

    const listRes = await fetch(
      `${LS_INSTANCES}?filter[license_key_id]=${encodeURIComponent(licenseKeyId)}&page[size]=100`,
      {
        headers: {
          'Authorization': `Bearer ${LS_API_KEY}`,
          'Accept': 'application/vnd.api+json',
        },
      }
    );
    if (!listRes.ok) return null;

    const listData = await listRes.json();
    const instances = Array.isArray(listData?.data) ? listData.data : [];
    // attributes.identifier is the UUID that the License API's /deactivate
    // expects as instance_id — NOT the numeric resource id.
    const stale = instances.filter(i => i?.attributes?.name === wantedName);
    if (stale.length === 0) return null;

    let freed = 0;
    for (const inst of stale) {
      const identifier = inst?.attributes?.identifier;
      if (!identifier) continue;
      const deRes = await fetch(`${LS_BASE}/deactivate`, {
        method: 'POST',
        headers: {
          'Authorization': `Bearer ${LS_API_KEY}`,
          'Content-Type': 'application/json',
          'Accept': 'application/json',
        },
        body: JSON.stringify({ license_key: licenseKey, instance_id: identifier }),
      });
      const deData = await deRes.json().catch(() => ({}));
      if (deRes.ok && deData.deactivated) freed++;
    }
    if (freed === 0) return null;

    const retryRes = await fetch(`${LS_BASE}/activate`, {
      method: 'POST',
      headers: {
        'Authorization': `Bearer ${LS_API_KEY}`,
        'Content-Type': 'application/json',
        'Accept': 'application/json',
      },
      body: JSON.stringify({ license_key: licenseKey, instance_name: wantedName }),
    });
    const retryData = await retryRes.json();
    if (retryRes.ok && retryData.activated) {
      return json({
        instance_id: retryData.instance.id,
        instance_name: retryData.instance.name,
        reclaimed: freed,
      });
    }
    return null;
  } catch {
    return null;
  }
}

async function handleDownloads() {
  const res = await fetch('https://api.github.com/repos/Doogals/AppLauncher/releases', {
    headers: { 'User-Agent': 'tonic-tech-worker' },
  });
  if (!res.ok) return json({ error: 'Failed to fetch' }, 500);
  const releases = await res.json();
  let total = 0;
  for (const release of releases) {
    for (const asset of release.assets) {
      if (asset.name.endsWith('.msi')) total += asset.download_count;
    }
  }
  return new Response(JSON.stringify({ total }), {
    headers: { 'Content-Type': 'application/json', 'Cache-Control': 'public, max-age=300', ...CORS_HEADERS },
  });
}

// ── Usage analytics ─────────────────────────────────────────────────────────

/// The ONLY fields stored. Anything else in the body is discarded.
///
/// This duplicates the allow-list the Rust client already enforces, on purpose:
/// a future client bug cannot start sending something it shouldn't without the
/// Worker being changed too. Two independent gates, not one.
const PING_COUNT_FIELDS = [
  'group_count',
  'item_count',
  'items_by_type',
  'startup_group_count',
  'detached_count',
  'positioned_item_count',
];

function isoWeek(date) {
  // ISO-8601: week 1 is the one containing the first Thursday.
  const d = new Date(Date.UTC(date.getUTCFullYear(), date.getUTCMonth(), date.getUTCDate()));
  d.setUTCDate(d.getUTCDate() + 4 - (d.getUTCDay() || 7));
  const yearStart = new Date(Date.UTC(d.getUTCFullYear(), 0, 1));
  const week = Math.ceil(((d - yearStart) / 86400000 + 1) / 7);
  return `${d.getUTCFullYear()}-W${String(week).padStart(2, '0')}`;
}

function sane(s, max) {
  return typeof s === 'string' && s.length > 0 && s.length <= max ? s : null;
}

async function handlePing(body) {
  const db = ANALYTICS;
  // No database bound yet — accept and drop, so a half-finished deploy never
  // makes the app look broken. The client ignores the response either way.
  if (!db) return json({ ok: true });

  const installId = sane(body.install_id, 64);
  if (!installId) return json({ error: 'bad install_id' }, 400);

  // The week is computed HERE, never taken from the client. A wrong clock on
  // someone's PC must not be able to write into another week's bucket.
  const week = isoWeek(new Date());

  const stats = {};
  for (const key of PING_COUNT_FIELDS) {
    const v = body[key];
    if (typeof v === 'number' && Number.isFinite(v)) stats[key] = v;
    // items_by_type is an object of numbers; copy only numeric members.
    else if (key === 'items_by_type' && v && typeof v === 'object') {
      const out = {};
      for (const [k, n] of Object.entries(v)) {
        if (typeof n === 'number' && Number.isFinite(n)) out[k] = n;
      }
      stats[key] = out;
    }
  }

  try {
    await db
      .prepare(
        `INSERT OR REPLACE INTO pings (install_id, week, version, licensed, os, stats)
         VALUES (?, ?, ?, ?, ?, ?)`
      )
      .bind(
        installId,
        week,
        sane(body.version, 20),
        body.licensed ? 1 : 0,
        sane(body.os, 40),
        JSON.stringify(stats)
      )
      .run();
  } catch {
    // Never surface a storage failure to the app.
    return json({ ok: true });
  }

  return json({ ok: true });
}

async function sendWeeklyReport() {
  const db = ANALYTICS;
  if (!db) return;

  // Report on the week that just ENDED. The cron fires Monday, so "now" is
  // already in the new week.
  const lastWeek = isoWeek(new Date(Date.now() - 3 * 86400000));
  const weekBefore = isoWeek(new Date(Date.now() - 10 * 86400000));

  const rows = (await db.prepare('SELECT * FROM pings WHERE week = ?').bind(lastWeek).all()).results || [];
  const prevCount =
    (await db.prepare('SELECT COUNT(*) AS n FROM pings WHERE week = ?').bind(weekBefore).first())?.n ?? 0;

  const total = rows.length;
  const licensed = rows.filter(r => r.licensed === 1).length;

  const byVersion = {};
  for (const r of rows) byVersion[r.version || 'unknown'] = (byVersion[r.version || 'unknown'] || 0) + 1;
  const versionLine = Object.entries(byVersion)
    .sort((a, b) => b[1] - a[1])
    .map(([v, n]) => `${v}: ${n}`)
    .join('    ') || '(none)';

  const stats = rows.map(r => { try { return JSON.parse(r.stats); } catch { return {}; } });
  const median = key => {
    const xs = stats.map(s => s[key]).filter(n => typeof n === 'number').sort((a, b) => a - b);
    return xs.length ? xs[Math.floor(xs.length / 2)] : 0;
  };
  const countWhere = pred => stats.filter(pred).length;
  const pct = (n, d) => (d ? ` (${Math.round((n / d) * 100)}%)` : '');

  const usingStartup = countWhere(s => (s.startup_group_count || 0) > 0);
  const usingDetached = countWhere(s => (s.detached_count || 0) > 0);
  const usingPlacement = countWhere(s => (s.positioned_item_count || 0) > 0);

  const delta = total - prevCount;
  const deltaStr = prevCount ? `  (${delta >= 0 ? '+' : ''}${delta} vs previous week)` : '';

  const text = [
    `TakeOff — ${lastWeek}`,
    '',
    `Active installs:  ${total}${deltaStr}`,
    `  Licensed:       ${licensed}`,
    `  Free:           ${total - licensed}`,
    '',
    `Versions:  ${versionLine}`,
    '',
    `Median groups per install:  ${median('group_count')}`,
    `Median items per install:   ${median('item_count')}`,
    '',
    `Using startup groups:   ${usingStartup}${pct(usingStartup, licensed)} of licensed`,
    `Using saved placement:  ${usingPlacement}${pct(usingPlacement, total)}`,
    `Detached a group:       ${usingDetached}${pct(usingDetached, total)}`,
  ].join('\n');

  await fetch('https://api.resend.com/emails', {
    method: 'POST',
    headers: {
      'Authorization': `Bearer ${RESEND_API_KEY}`,
      'Content-Type': 'application/json',
    },
    body: JSON.stringify({
      from: 'TakeOff Analytics <onboarding@resend.dev>',
      to: FEEDBACK_TO,
      subject: `TakeOff weekly — ${total} active`,
      text,
    }),
  });

  // Retention: per-install rows older than ~26 weeks serve no question being
  // asked, and keeping them would be hard to defend.
  const cutoff = isoWeek(new Date(Date.now() - 26 * 7 * 86400000));
  try {
    await db.prepare('DELETE FROM pings WHERE week < ?').bind(cutoff).run();
  } catch { /* pruning is best-effort */ }
}

function json(data, status = 200) {
  return new Response(JSON.stringify(data), {
    status,
    headers: { 'Content-Type': 'application/json', ...CORS_HEADERS },
  });
}

// btoa() throws on any code point above U+00FF, and group names and file paths
// routinely contain accented characters and emoji. Encode to UTF-8 bytes first.
function base64Utf8(str) {
  const bytes = new TextEncoder().encode(str);
  let binary = '';
  // Chunked because String.fromCharCode(...bytes) blows the argument limit on
  // anything larger than a few tens of kilobytes.
  const CHUNK = 0x8000;
  for (let i = 0; i < bytes.length; i += CHUNK) {
    binary += String.fromCharCode.apply(null, bytes.subarray(i, i + CHUNK));
  }
  return btoa(binary);
}

// Loose sanity check, not RFC validation. The only job here is to avoid handing
// Resend something it will reject outright; anything that gets past this and is
// still wrong just bounces when you hit Reply, which is recoverable.
function looksLikeEmail(s) {
  return typeof s === 'string'
    && s.length > 2
    && s.length <= 254
    && /^[^\s@,;<>]+@[^\s@,;<>]+\.[^\s@,;<>]+$/.test(s);
}

// The machine label is user-influenced (it embeds COMPUTERNAME), so it is not
// trusted directly in a filename.
function safeFilePart(s) {
  return (s || 'unknown').replace(/[^a-zA-Z0-9._-]+/g, '-').slice(0, 60) || 'unknown';
}
