'use client';

import { useState } from 'react';
import { useT } from '@/components/Prefs';
import { send } from '@/lib/api';

export default function ResourceAgents({ preset, live, refresh, native }) {
  const t = useT();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);
  if (native) return <div data-testid={`resource-agents-${preset.id}`}><button className="btn" disabled={!native.allowed || native.busy} onClick={() => native.publish(preset, !preset.published)}>{t(preset.published ? 'nr.withdraw' : 'nr.publish')}</button></div>;
  return <div data-testid={`resource-agents-${preset.id}`}>
    <p><button className="btn" disabled={!live || busy} onClick={async () => {
      setBusy(true); setError(null);
      const result = await send(`framework-presets/${encodeURIComponent(preset.id)}/catalog`, { method: 'PUT', body: { published: !preset.catalogPublished } });
      setBusy(false); if (!result.ok) setError(result.error); else await refresh();
    }}>{t(preset.catalogPublished ? 'rs.unpublishCatalog' : 'rs.publishCatalog')}</button></p>
    {error && <p role="alert" className="warn-text">{error}</p>}
  </div>;
}
