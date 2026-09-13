'use client';

import { useData } from '@/components/Data';
import NativeProjectSides from '@/components/NativeProjectSides';

/*
 * The native project-sides route (ADR-132). Deliberately a separate path
 * from the retained /projects: that page also renders invites, whitelist
 * and contributions, which a native console has no source for, so folding
 * a native branch into it would hide most of the page. This route exists
 * only in the native build (NATIVE_MODE), so there is no retained arm.
 */
export default function ProjectSidesPage() {
  const data = useData();
  return data.nativeConsole ? <NativeProjectSides /> : null;
}
