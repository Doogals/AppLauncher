const LS_BASE = 'https://api.lemonsqueezy.com/v1/licenses';

addEventListener('fetch', event => {
  event.respondWith(handleRequest(event.request));
});

async function handleRequest(request) {
  if (request.method !== 'POST') {
    return new Response('Method not allowed', { status: 405 });
  }

  const url = new URL(request.url);
  const action = url.pathname.slice(1); // 'activate', 'deactivate', or 'validate'

  if (!['activate', 'deactivate', 'validate'].includes(action)) {
    return new Response('Not found', { status: 404 });
  }

  let body;
  try {
    body = await request.json();
  } catch {
    return new Response(JSON.stringify({ error: 'Invalid JSON body' }), {
      status: 400,
      headers: { 'Content-Type': 'application/json' },
    });
  }

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

function json(data, status = 200) {
  return new Response(JSON.stringify(data), {
    status,
    headers: { 'Content-Type': 'application/json' },
  });
}
