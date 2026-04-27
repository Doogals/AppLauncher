const LS_BASE = 'https://api.lemonsqueezy.com/v1/licenses';

export default {
  async fetch(request, env) {
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
      return Response.json({ error: 'Invalid JSON body' }, { status: 400 });
    }

    const lsRes = await fetch(`${LS_BASE}/${action}`, {
      method: 'POST',
      headers: {
        'Authorization': `Bearer ${env.LS_API_KEY}`,
        'Content-Type': 'application/json',
        'Accept': 'application/json',
      },
      body: JSON.stringify(body),
    });

    const data = await lsRes.json();

    if (action === 'activate') {
      if (lsRes.ok && data.activated) {
        return Response.json({
          instance_id: data.instance.id,
          instance_name: data.instance.name,
        });
      }
      return Response.json(
        { error: data.error || data.errors?.[0]?.detail || 'Activation failed' },
        { status: 400 }
      );
    }

    if (action === 'deactivate') {
      if (lsRes.ok && data.deactivated) {
        return Response.json({ ok: true });
      }
      return Response.json(
        { error: data.error || 'Deactivation failed' },
        { status: 400 }
      );
    }

    // validate
    return Response.json({ valid: lsRes.ok && data.valid === true });
  },
};
