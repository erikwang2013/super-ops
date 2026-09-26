import { beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from './api';
import { useAuthStore } from '../stores/auth';

function jsonResponse(status: number, body: unknown) {
  return {
    ok: status < 400,
    status,
    json: async () => body,
  } as Response;
}

describe('api request layer', () => {
  beforeEach(() => {
    useAuthStore.setState({ token: null, refreshToken: null, username: null, isAuthenticated: false });
    vi.unstubAllGlobals();
  });

  it('prefixes /api and sends Bearer token from auth store', async () => {
    useAuthStore.getState().login('tok-1', 'erik');
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse(200, { clusters: [] }));
    vi.stubGlobal('fetch', fetchMock);

    await api.get('/k8s/clusters');

    const [url, init] = fetchMock.mock.calls[0];
    expect(String(url)).toBe('/api/k8s/clusters');
    const headers = (init?.headers ?? {}) as Record<string, string>;
    expect(headers['Authorization']).toBe('Bearer tok-1');
    expect(headers['Content-Type']).toBe('application/json');
  });

  it('logs out and throws on 401 without refresh token', async () => {
    useAuthStore.getState().login('tok', 'erik');
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(jsonResponse(401, { error: 'unauthorized' })));

    await expect(api.get('/k8s/clusters')).rejects.toThrow('未授权');
    expect(useAuthStore.getState().isAuthenticated).toBe(false);
    expect(useAuthStore.getState().token).toBeNull();
  });

  it('refreshes once then retries the original request on 401', async () => {
    useAuthStore.getState().login('old-tok', 'erik', 'ref-1');
    const fetchMock = vi.fn()
      .mockResolvedValueOnce(jsonResponse(401, { error: 'expired' }))
      .mockResolvedValueOnce(jsonResponse(200, { access_token: 'new-tok', refresh_token: 'ref-2' }))
      .mockResolvedValueOnce(jsonResponse(200, { clusters: [] }));
    vi.stubGlobal('fetch', fetchMock);

    await expect(api.get('/k8s/clusters')).resolves.toEqual({ clusters: [] });
    expect(useAuthStore.getState().token).toBe('new-tok');
    expect(useAuthStore.getState().refreshToken).toBe('ref-2');
    expect(String(fetchMock.mock.calls[1][0])).toBe('/api/auth/refresh');
    expect(String(fetchMock.mock.calls[2][0])).toBe('/api/k8s/clusters');
    const retryHeaders = (fetchMock.mock.calls[2][1]?.headers ?? {}) as Record<string, string>;
    expect(retryHeaders['Authorization']).toBe('Bearer new-tok');
  });

  it('surfaces backend error message with status', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(jsonResponse(400, { error: 'bad request' })));

    await expect(api.post('/cmdb/assets', {})).rejects.toMatchObject({
      name: 'ApiError',
      status: 400,
      message: 'bad request',
    });
  });

  it('returns parsed json on success and undefined on 204', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse(200, { ok: true }));
    vi.stubGlobal('fetch', fetchMock);
    await expect(api.post('/k8s/clusters/1/restart', {})).resolves.toEqual({ ok: true });

    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, status: 204, json: async () => { throw new Error('no body'); } } as unknown as Response));
    await expect(api.delete('/k8s/clusters/1')).resolves.toBeUndefined();
  });
});
