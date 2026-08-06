import { useAuthStore } from '../stores/auth';

const BASE_URL = '/api';

class ApiError extends Error {
  status: number;
  constructor(message: string, status: number) {
    super(message); this.status = status; this.name = 'ApiError';
  }
}

async function request<T>(path: string, options: RequestInit = {}): Promise<T> {
  const token = useAuthStore.getState().token;
  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
    ...(options.headers as Record<string, string> || {}),
  };
  if (token) headers['Authorization'] = `Bearer ${token}`;

  const response = await fetch(`${BASE_URL}${path}`, { ...options, headers });
  if (response.status === 401) {
    useAuthStore.getState().logout();
    throw new ApiError('未授权，请重新登录', 401);
  }
  if (!response.ok) {
    const body = await response.json().catch(() => ({}));
    throw new ApiError(body.error || '请求失败', response.status);
  }
  if (response.status === 204) return undefined as T;
  return response.json();
}

export const api = {
  get: <T>(path: string) => request<T>(path),
  post: <T>(path: string, body: unknown) => request<T>(path, { method: 'POST', body: JSON.stringify(body) }),
  patch: <T>(path: string, body: unknown) => request<T>(path, { method: 'PATCH', body: JSON.stringify(body) }),
  delete: <T>(path: string) => request<T>(path, { method: 'DELETE' }),
};

export interface AuditEvent { timestamp: string; event_type: string; username: string; ip: string; detail: string; }
export interface ApiKeyRow { id: string; name: string; }
export interface ApiKeyCreated { id: string; name: string; key: string; warning: string; }
export interface UserRow { id: string; username: string; email: string; role: string; status: string; created_at: string; }
export interface AssetRow { id: number; asset_type: string; name: string; ip?: string; env: string; owner: string; labels?: string; status: string; }
export interface CmdbAssetInput { asset_type: string; name: string; ip?: string; env?: string; owner?: string; labels?: string; }
export interface CmdbStats { hosts: number; switches: number; router: number; app: number; db: number; storage: number; }
export interface ScriptItem { id: number; name: string; description: string; language: string; timeout_s: number; created_by: string; created_at?: string }
export interface ScriptRun { id: number; script_id: number; target_pods: string; status: string; output?: string; started_at?: string; finished_at?: string }
export interface ScriptInput { name: string; description?: string; language: string; content: string; timeout_s?: number }
export interface ScriptRunInput { cluster_id: string; namespace: string; target_pods: string[] }

export const opsApi = {
  getAuditEvents: (limit: number, offset: number, level?: string) =>
    api.get<{ events: AuditEvent[] }>(`/audit/events?limit=${limit}&offset=${offset}&level=${level || ''}`),
  listApiKeys: () => api.get<{ keys: ApiKeyRow[] }>('/keys'),
  createApiKey: (name: string) => api.post<ApiKeyCreated>('/keys', { name }),
  deleteApiKey: (id: string) => api.delete<undefined>(`/keys/${id}`),
  listUsers: () => api.get<{ users: UserRow[] }>('/users'),
  setUserStatus: (id: string, status: string) => api.patch<undefined>(`/users/${id}/status`, { status }),
};

export const cmdbApi = {
  listAssets: (assetType?: string, env?: string) =>
    api.get<{ assets: AssetRow[] }>(`/cmdb/assets?asset_type=${assetType || ''}&env=${env || ''}`),
  createAsset: (body: CmdbAssetInput) =>
    api.post<{ id: number }>('/cmdb/assets', body),
  deleteAsset: (id: number) => api.delete<undefined>(`/cmdb/assets/${id}`),
  getCmdbStats: () => api.get<CmdbStats>('/cmdb/stats'),
};

export const scriptApi = {
  listScripts: () => api.get<{ scripts: ScriptItem[] }>('/scripts'),
  createScript: (body: ScriptInput) => api.post<{ id: number }>('/scripts', body),
  deleteScript: (id: number) => api.delete<undefined>(`/scripts/${id}`),
  runScript: (id: number, body: ScriptRunInput) =>
    api.post<{ run_id: number; status: string }>(`/scripts/${id}/run`, body),
  listRuns: (scriptId?: number) =>
    api.get<{ runs: ScriptRun[] }>(`/scripts/runs?script_id=${scriptId || ''}&limit=50`),
};

export interface AlertRow { id: number; level: string; title: string; message: string; }

export const alertApi = {
  listAlerts: (level?: string) =>
    api.get<{ alerts: AlertRow[] }>(`/alerts?limit=50&level=${level || ''}`),
  ackAlert: (id: number) => api.post<{ ok: boolean }>(`/alerts/${id}/ack`, {}),
  listAcks: () => api.get<{ ids: number[] }>('/alerts/acks'),
};
export { ApiError };
