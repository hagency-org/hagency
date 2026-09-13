'use client';

import { useEffect, useState } from 'react';
import NativeAccounts from '@/components/NativeAccounts';
import { exchangeAccess, fetchAccounts } from '@/lib/native-api';

/* The native-only accounts page: no retained counterpart exists. A
 * non-document with no query string, deliberately outside the console's
 * five-document exception — a foreign origin cannot even navigate here. */
export default function AccountsPage() {
  const [state, setState] = useState({ phase: 'loading', accounts: null });

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        await exchangeAccess(window.location, window.history);
        const value = await fetchAccounts();
        if (!cancelled) setState({ phase: 'ready', accounts: value.accounts });
      } catch {
        if (!cancelled) setState({ phase: 'access', accounts: null });
      }
    })();
    return () => { cancelled = true; };
  }, []);

  return <NativeAccounts phase={state.phase} accounts={state.accounts} />;
}
