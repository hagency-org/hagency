import { describe, expect, test } from 'vitest';
import { renderDashboard } from './helpers/dashboard-render.js';
import { selection, validateEngagements, validateReport } from '../mockup/lib/native-api.js';

const counts = { input: 4, output: 1, cacheWrite: 0, cacheRead: 1 };
const report = { engagement_id: 'created_after_build', at_ms: 2000, summary: { sources: 1, latest_counts: counts, known_high_water_lower_bound: { ...counts, input: 7 }, latest_incomplete_sources: 0, historically_incomplete_sources: 1, regression_observations: 1, evidence: 'host_attributed_untrusted_usage' }, daily: null, monthly: null };
describe('retained usage native mode', () => {
  test('native observations preserve nulls lower bounds and evidence', async () => {
    expect(validateReport(report, 'created_after_build')).toBe(report);
    for (const bad of [{ ...report, engagement_id: 'wrong' }, { ...report, billingVerified: true }, { ...report, summary: { ...report.summary, latest_counts: { ...counts, input: NaN } } }]) expect(() => validateReport(bad, 'created_after_build')).toThrow();
    const html = await renderDashboard('mockup/app/usage/page.jsx', { data: { nativeConsole: true, phase: 'ready', report, engagements: [], selected: report.engagement_id } });
    expect(html).toContain('Historical high-water lower bounds');
    expect(html).toContain('No observation for this period');
    expect(html).not.toContain('NaN');
  });
  test('unknown native state cannot become a fixture or zero response', async () => {
    const html = await renderDashboard('mockup/app/usage/page.jsx', { data: { nativeConsole: true, phase: 'access', report: null, engagements: [], selected: null } });
    expect(html).toContain('Console access required');
    expect(html).not.toContain('data-kind');
    expect(() => selection({ search: '?data=fixture' })).toThrow();
    expect(() => selection({ search: '?engagement_id=a&engagement_id=b' })).toThrow();
    expect(selection({ search: '?engagement_id=created_after_build' })).toBe('created_after_build');
    expect(() => validateEngagements({ engagements: [], next_after: null, token: 'not_allowed' })).toThrow();
  });
});
