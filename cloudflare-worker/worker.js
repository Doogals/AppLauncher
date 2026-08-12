const LS_BASE = 'https://api.lemonsqueezy.com/v1/licenses';
// Main API (not the License API) — used to look up a key's existing activations
// so a stale one from the same machine can be reclaimed. Requires an API key
// with access to the license-key-instances endpoint.
const LS_INSTANCES = 'https://api.lemonsqueezy.com/v1/license-key-instances';
// @ts-ignore — injected by Cloudflare as environment variables
const LS_API_KEY = globalThis['LS_API_KEY'];
const RESEND_API_KEY = globalThis['RESEND_API_KEY'];
const FEEDBACK_TO = 'tonictech.inquiry@gmail.com';

const CORS_HEADERS = {
  'Access-Control-Allow-Origin': '*',
  'Access-Control-Allow-Methods': 'GET, POST, OPTIONS',
  'Access-Control-Allow-Headers': 'Content-Type',
};

addEventListener('fetch', event => {
  event.respondWith(handleRequest(event.request));
});

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

  if (!['activate', 'deactivate', 'validate', 'feedback'].includes(action)) {
    return new Response('Not found', { status: 404, headers: CORS_HEADERS });
  }

  let body;
  try {
    body = await request.json();
  } catch {
    return json({ error: 'Invalid JSON body' }, 400);
  }

  // Handle feedback separately — does not touch LemonSqueezy
  if (action === 'feedback') {
    const res = await fetch('https://api.resend.com/emails', {
      method: 'POST',
      headers: {
        'Authorization': `Bearer ${RESEND_API_KEY}`,
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({
        from: 'Tonic Tech Feedback <onboarding@resend.dev>',
        to: FEEDBACK_TO,
        subject: 'Tonic Tech Feedback',
        text: body.message || '(no message)',
      }),
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

function json(data, status = 200) {
  return new Response(JSON.stringify(data), {
    status,
    headers: { 'Content-Type': 'application/json', ...CORS_HEADERS },
  });
}
