import { useAuthStore } from '../stores/auth';

const BASE_URL = '/api';

class ApiError extends Error {
  status: number;
  constructor(message: string, status: number) {
    super(message); this.status = status; this.name = 'ApiError';
  }
}

async function tryRefresh(): Promise<boolean> {
  const { refreshToken, username } = useAuthStore.getState();
  if (!refreshToken) return false;
  const response = await fetch(`${BASE_URL}/auth/refresh`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ refresh_token: refreshToken }),
  });
  if (!response.ok) return false;
  const body = await response.json().catch(() => ({}));
  if (!body.access_token) return false;
  useAuthStore.getState().login(
    body.access_token,
    username || '',
    body.refresh_token || refreshToken,
  );
  return true;
}

async function request<T>(path: string, options: RequestInit = {}, retried = false): Promise<T> {
  const token = useAuthStore.getState().token;
  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
    ...(options.headers as Record<string, string> || {}),
  };
  if (token) headers['Authorization'] = `Bearer ${token}`;

  const response = await fetch(`${BASE_URL}${path}`, { ...options, headers });
  if (response.status === 401) {
    const skipRefresh = path.includes('/auth/refresh') || path.includes('/auth/login');
    if (!retried && !skipRefresh && await tryRefresh()) {
      return request<T>(path, options, true);
    }
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
  put: <T>(path: string, body: unknown) => request<T>(path, { method: 'PUT', body: JSON.stringify(body) }),
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

export interface TopoNode {
  key: string; asset_type: string; name: string;
  ip?: string; env: string; owner: string; status: string;
}
export interface TopoEdge { src: string; dst: string; }
export interface TopologyData { provider: string; nodes: TopoNode[]; edges: TopoEdge[]; }

export const topologyApi = {
  get: () => api.get<TopologyData>('/cmdb/topology'),
  sync: () => api.post<{ synced: number; edges: number; provider: string }>('/cmdb/topology/sync', {}),
};

export interface DomainEventRow {
  event_type: string; level: string; title: string; message: string; ts: string;
}
export const eventApi = {
  list: (limit = 100, eventType?: string) =>
    api.get<{ events: DomainEventRow[] }>(`/events?limit=${limit}&event_type=${eventType || ''}`),
};

export interface QuotaRow {
  id: number;
  cluster_id: string;
  namespace: string;
  cpu_request: string;
  memory_request: string;
  cpu_limit: string;
  memory_limit: string;
  replicas: number;
  description: string;
  created_at: string;
}

export interface QuotaInput {
  cluster_id?: string;
  namespace: string;
  cpu_request?: string;
  memory_request?: string;
  cpu_limit?: string;
  memory_limit?: string;
  replicas?: number;
  description?: string;
}

export const quotaApi = {
  listQuotas: () => api.get<{ quotas: QuotaRow[] }>('/quota'),
  createQuota: (body: QuotaInput) => api.post<{ id: number }>('/quota', body),
  deleteQuota: (id: number) => api.delete<undefined>(`/quota/${id}`),
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

// ---- 日志检索 ----
export interface LogRow { namespace: string; pod: string; content: string; ts: string; }
export interface LogSearchParams {
  namespace?: string; pod?: string; keyword?: string; from?: number; to?: number; limit?: number;
}

export const logApi = {
  searchLogs: (p: LogSearchParams) => {
    const qs = [
      p.namespace ? `namespace=${encodeURIComponent(p.namespace)}` : '',
      p.pod ? `pod=${encodeURIComponent(p.pod)}` : '',
      p.keyword ? `keyword=${encodeURIComponent(p.keyword)}` : '',
      p.from ? `from=${p.from}` : '',
      p.to ? `to=${p.to}` : '',
      `limit=${p.limit || 100}`,
    ].filter(Boolean).join('&');
    return api.get<{ logs: LogRow[] }>(`/logs/search?${qs}`);
  },
};

// ---- 终端录制 ----
export interface RecordingRow { session_id: string; created_at: number; frames: number; }
export interface FrameRow { ts: number; seq: number; data: string; }

export const recordingApi = {
  listRecordings: (limit = 50) =>
    api.get<{ recordings: RecordingRow[] }>(`/recordings?limit=${limit}`),
  getFrames: (sid: string) =>
    api.get<{ session_id: string; frames: FrameRow[] }>(`/recordings/${sid}/frames`),
  deleteRecording: (sid: string) => api.delete<undefined>(`/recordings/${sid}`),
};

// ---- 审批 ----
export interface ApprovalRow {
  id: number; kind: string; target: string; operator: string; reason: string;
  status: string; created_at: string; decided_by?: string; decided_at?: string;
}

export const approvalApi = {
  listApprovals: (status?: string, limit = 50) =>
    api.get<{ approvals: ApprovalRow[] }>(`/approvals?status=${status || ''}&limit=${limit}`),
  createApproval: (body: { kind: string; target: string; reason?: string }) =>
    api.post<{ id: number; status: string }>('/approvals', body),
  decideApproval: (id: number, action: string) =>
    api.post<ApprovalRow>(`/approvals/${id}/decide`, { action }),
};

// ---- 保险库 ----
export interface SecretRow { name: string; created_at: string; }

export const secretApi = {
  listSecrets: () => api.get<SecretRow[]>('/secrets'),
  createSecret: (name: string, value: string) =>
    api.post<{ name: string; created_at: string }>('/secrets', { name, value }),
  getSecret: (name: string) =>
    api.get<{ value: string }>(`/secrets/${encodeURIComponent(name)}`),
  deleteSecret: (name: string) => api.delete<undefined>(`/secrets/${encodeURIComponent(name)}`),
};

// ---- 文件（multipart 上传 / 原始字节下载，不走 JSON 包装） ----
export const fileApi = {
  upload: async (file: File): Promise<{ name: string }> => {
    const token = useAuthStore.getState().token;
    const fd = new FormData();
    fd.append('file', file);
    const res = await fetch(`${BASE_URL}/files`, {
      method: 'POST', body: fd,
      headers: token ? { Authorization: `Bearer ${token}` } : {},
    });
    if (!res.ok) throw new ApiError((await res.json().catch(() => ({}))).error || '上传失败', res.status);
    return res.json();
  },
  download: async (name: string): Promise<Blob> => {
    const token = useAuthStore.getState().token;
    const res = await fetch(`${BASE_URL}/files/${encodeURIComponent(name)}`, {
      headers: token ? { Authorization: `Bearer ${token}` } : {},
    });
    if (!res.ok) throw new ApiError((await res.json().catch(() => ({}))).error || '下载失败', res.status);
    return res.blob();
  },
};
export { ApiError };

// ---- 告警规则 ----
export interface AlertRuleRow {
  id: number; name: string; metric: string; operator: string;
  threshold: string; level: string; action: string; enabled: boolean;
}

export const alertRuleApi = {
  listRules: () => api.get<{ rules: AlertRuleRow[] }>('/alert-rules'),
  createRule: (body: {
    name: string; metric: string; operator: string; threshold: string;
    level: string; action: string; enabled?: boolean;
  }) => api.post<{ id: number }>('/alert-rules', body),
  setEnabled: (id: number, enabled: boolean) =>
    api.patch<undefined>(`/alert-rules/${id}/status`, { enabled }),
  deleteRule: (id: number) => api.delete<undefined>(`/alert-rules/${id}`),
};

// ---- 值班排班 ----
export interface OncallShiftRow {
  id: number; name: string; assignee: string;
  start_at: string; end_at: string; created_at: string;
}

export const oncallApi = {
  listShifts: () => api.get<{ shifts: OncallShiftRow[] }>('/oncall/shifts'),
  currentShift: () =>
    api.get<{ shift: OncallShiftRow | null; message?: string }>('/oncall/shifts/current'),
  createShift: (body: {
    name: string; assignee: string; start_at: string; end_at: string;
  }) => api.post<{ id: number }>('/oncall/shifts', body),
  deleteShift: (id: number) => api.delete<undefined>(`/oncall/shifts/${id}`),
};

// ---- 工单 ----
export interface TicketRow {
  id: number; title: string; description: string | null; severity: string;
  status: string; assignee: string; source: string; alert_title: string;
  created_by: string; created_at: string; updated_at: string;
}

export const ticketApi = {
  listTickets: (status?: string) =>
    api.get<{ tickets: TicketRow[] }>(`/tickets${status ? `?status=${status}` : ''}`),
  createTicket: (body: {
    title: string; description?: string; severity?: string;
    source?: string; alert_title?: string;
  }) => api.post<{ id: number }>('/tickets', body),
  setStatus: (id: number, status: string, assignee?: string) =>
    api.patch<undefined>(`/tickets/${id}/status`, { status, assignee }),
  deleteTicket: (id: number) => api.delete<undefined>(`/tickets/${id}`),
};

// ---- 发布流水线 ----
export interface ReleaseRow {
  id: number; cluster_id: string; namespace: string; name: string;
  old_image: string; new_image: string; operator: string;
  status: string; created_at: string;
}

export const releaseApi = {
  listReleases: (status?: string) =>
    api.get<{ releases: ReleaseRow[] }>(`/releases${status ? `?status=${status}` : ''}`),
  createRelease: (body: {
    cluster_id?: string; namespace: string; name: string;
    old_image?: string; new_image: string; operator?: string;
  }) => api.post<{ id: number; image: string; status: string }>('/releases', body),
  rollbackRelease: (id: number, operator?: string) =>
    api.post<{ id: number; image: string; status: string }>(`/releases/${id}/rollback`, { operator: operator || '' }),
};

// ---- 混沌演练 ----
export interface ChaosRow {
  id: number; name: string; cluster_id: string; target_type: string;
  target_name: string; action: string; status: string; operator: string;
  error?: string | null; started_at?: string | null; ended_at?: string | null; created_at: string;
}
export const chaosApi = {
  listExperiments: () => api.get<{ experiments: ChaosRow[] }>('/chaos'),
  createExperiment: (body: {
    name: string; cluster_id?: string; target_name: string; action: string; operator?: string;
  }) => api.post<{ id: number }>('/chaos', body),
  runExperiment: (id: number) => api.post<{ status: string }>(`/chaos/${id}/run`, {}),
  deleteExperiment: (id: number) => api.delete(`/chaos/${id}`),
};

// ---- Runbook 剧本 ----
export interface RunbookStep { name: string; script_id: number; timeout_s: number; }
export interface RunbookRow {
  id: number; name: string; description: string; steps: string;
  created_by: string; created_at: string;
}
export interface RunbookRunRow {
  id: number; runbook_id: number; runbook_name: string; target_pods: string;
  status: string; output?: string; started_at: string; finished_at?: string;
}

export const runbookApi = {
  listRunbooks: () => api.get<{ runbooks: RunbookRow[] }>('/runbooks'),
  createRunbook: (body: { name: string; description?: string; steps: string }) =>
    api.post<{ id: number }>('/runbooks', body),
  deleteRunbook: (id: number) => api.delete<undefined>(`/runbooks/${id}`),
  runRunbook: (id: number, body: {
    cluster_id: string; namespace: string; target_pods: string[];
  }) => api.post<{ run_id: number; status: string; steps?: number }>(`/runbooks/${id}/run`, body),
  listRuns: (limit = 50) => api.get<{ runs: RunbookRunRow[] }>(`/runbooks/runs?limit=${limit}`),
};

// ---- 容量/成本 ----
export interface CapacitySummary {
  cpu_cores: number; mem_gib: number; node_count: number;
  cost_yuan_day: number; cost_yuan_month: number;
}
export interface CapacityPoint {
  bucket: string; cpu_cores: number; mem_gib: number;
  node_count: number; cost_yuan_day: number;
}

export const capacityApi = {
  summary: () => api.get<CapacitySummary>('/capacity/summary'),
  trend: (hours = 24) => api.get<{ points: CapacityPoint[] }>(`/capacity/trend?hours=${hours}`),
};

// ---- DB 备份状态 ----
export interface BackupStatusRow {
  id: number; db_name: string; target: string; status: string;
  size_bytes: number; message: string; started_at: string; finished_at?: string;
}
export interface BackupSummaryRow {
  db_name: string; status: string; target: string; size_bytes: number;
  last_ok_at: string; age_hours: number;
}

export const backupApi = {
  listStatus: (limit = 50) => api.get<{ backups: BackupStatusRow[] }>(`/backups/status?limit=${limit}`),
  summary: () => api.get<{ summaries: BackupSummaryRow[] }>('/backups/summary'),
  listObjects: () => api.get<{ objects: string[]; provider: string }>('/backups/objects'),
};

export interface ConfigKeyRow { key: string; value: string; modified_index: number; }
export const configRemoteApi = {
  listKeys: () => api.get<{ keys: ConfigKeyRow[] }>('/config/remote/keys'),
  getKey: (key: string) => api.get<{ key: string; value: string }>(`/config/remote/keys/${key}`),
  putKey: (key: string, value: string) => api.put(`/config/remote/keys/${key}`, { value }),
  deleteKey: (key: string) => api.delete(`/config/remote/keys/${key}`),
};
