// 見本の記録を、公の API で置く（ほかの試験より先に 1 度だけ。browser の外から送るので Origin は付かない）。
//   pm.1（人）─起動した→ tpm.1（opus-5-5 · high）─起動した→ se.1（指定は opus、実際は sonnet-5 · high。/clear で退く）
//   se.1 ─引き継いだ→ se.2（opus-5-5 · high）
//   tpm.1 ─起動した→ other.1（別の Key の t-99。haiku-5 · low。退いた）
// 点は 5 つ、線は 4 本（起動した 3・引き継いだ 1）。
import { expect, test as setup, type APIRequestContext } from '@playwright/test';

const now = () => new Date().toISOString();

async function send(request: APIRequestContext, method: 'post' | 'put', path: string, data: object): Promise<void> {
  const response = await request[method](path, { data });
  expect(response.ok(), `${path}: ${await response.text()}`).toBeTruthy();
}

function seat(prefix: string, session: string, key: string, model: string, effort: string): object {
  return {
    issuer: 'tpm',
    kind: 'ai',
    name_prefix: prefix,
    labels: { project: 'castlist', 'tpm.key': key },
    bindings: [{ type: 'claude.session', value: session }],
    settings: { model, effort },
    at: now(),
  };
}

const event = (by: string, type: string) => ({ issuer: 'tpm', by, type, at: now() });
const line = (type: string, from: string, to: string) => ({ issuer: 'tpm', type, from, to, at: now() });

setup('見本の記録を置く', async ({ request }) => {
  const PM = 'tpm.person:pm';
  const TPM = 'claude.session:s-tpm';
  const SE1 = 'claude.session:s-se1';
  const SE2 = 'claude.session:s-se2';
  const OTHER = 'claude.session:s-other';
  await send(request, 'post', '/v1/actors', {
    issuer: 'tpm',
    kind: 'human',
    name_prefix: 'pm',
    labels: { project: 'castlist' },
    bindings: [{ type: 'tpm.person', value: 'pm' }],
    at: now(),
  });
  await send(request, 'put', '/v1/events/e-pm-in', event(PM, 'entered'));
  await send(request, 'post', '/v1/actors', seat('tpm', 's-tpm', 't-23zs', 'claude-opus-5-5', 'high'));
  await send(request, 'put', '/v1/links/l-pm-tpm', line('launched', PM, TPM));
  await send(request, 'put', '/v1/events/e-tpm-in', event(TPM, 'entered'));
  await send(request, 'post', '/v1/actors', seat('se', 's-se1', 't-23zs', 'claude-opus-5-5', 'high'));
  await send(request, 'put', '/v1/links/l-tpm-se1', line('launched', TPM, SE1));
  await send(request, 'put', '/v1/events/e-se1-in', event(SE1, 'entered'));
  await send(request, 'put', '/v1/settings/t-se1-sonnet', {
    issuer: 'tpm',
    by: SE1,
    model: 'claude-sonnet-5',
    effort: 'high',
    source: 'observed',
    at: now(),
  });
  await send(request, 'put', '/v1/events/e-se1-clear', event(SE1, 'cleared'));
  await send(request, 'post', '/v1/actors', seat('se', 's-se2', 't-23zs', 'claude-opus-5-5', 'high'));
  await send(request, 'put', '/v1/links/l-se1-se2', line('continued', SE1, SE2));
  await send(request, 'put', '/v1/events/e-se2-in', event(SE2, 'entered'));
  await send(request, 'post', '/v1/actors', seat('other', 's-other', 't-99', 'claude-haiku-5', 'low'));
  await send(request, 'put', '/v1/links/l-tpm-other', line('launched', TPM, OTHER));
  await send(request, 'put', '/v1/events/e-other-in', event(OTHER, 'entered'));
  await send(request, 'put', '/v1/events/e-other-out', event(OTHER, 'exited'));

  const graph = await (await request.get('/v1/graph')).json();
  expect(graph.nodes.map((n: { name: string }) => n.name).sort()).toEqual(['other.1', 'pm.1', 'se.1', 'se.2', 'tpm.1']);
  expect(graph.edges).toHaveLength(4);
});
