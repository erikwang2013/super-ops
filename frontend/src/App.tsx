import { lazy, Suspense } from 'react';
import { ProLayout, PageContainer } from '@ant-design/pro-components';
import { Routes, Route, useNavigate, useLocation, Link } from 'react-router-dom';
import { DashboardOutlined, CloudServerOutlined, DatabaseOutlined, SettingOutlined } from '@ant-design/icons';
import { useAuthStore } from './stores/auth';
import LoginPage from './pages/login';
import SuperPet from './components/super-pet';

const Dashboard = lazy(() => import('./pages/dashboard'));
const ClustersPage = lazy(() => import('./pages/k8s/clusters'));
const ClusterDetail = lazy(() => import('./pages/k8s/cluster-detail'));
const PodsPage = lazy(() => import('./pages/k8s/pods'));
const DeploymentsPage = lazy(() => import('./pages/k8s/deployments'));
const NodesPage = lazy(() => import('./pages/k8s/nodes'));
const TerminalPage = lazy(() => import('./pages/k8s/terminal'));
const AuditPage = lazy(() => import('./pages/ops/audit'));
const EventsPage = lazy(() => import('./pages/ops/events'));
const ApiKeysPage = lazy(() => import('./pages/ops/apikeys'));
const UsersPage = lazy(() => import('./pages/ops/users'));
const CmdbPage = lazy(() => import('./pages/cmdb'));
const ScriptsPage = lazy(() => import('./pages/ops/scripts'));
const AlertsPage = lazy(() => import('./pages/ops/alerts'));
const AlertRulesPage = lazy(() => import('./pages/ops/alert-rules'));
const MetricsPage = lazy(() => import('./pages/ops/metrics'));
const LogsPage = lazy(() => import('./pages/ops/logs'));
const RecordingsPage = lazy(() => import('./pages/ops/recordings'));
const ApprovalsPage = lazy(() => import('./pages/ops/approvals'));
const SecretsPage = lazy(() => import('./pages/ops/secrets'));
const FilesPage = lazy(() => import('./pages/ops/files'));
const OncallPage = lazy(() => import('./pages/ops/oncall'));
const TracesPage = lazy(() => import('./pages/ops/traces'));
const TicketsPage = lazy(() => import('./pages/ops/tickets'));
const ReleasesPage = lazy(() => import('./pages/ops/releases'));
const RunbooksPage = lazy(() => import('./pages/ops/runbooks'));
const CapacityPage = lazy(() => import('./pages/ops/capacity'));
const BackupsPage = lazy(() => import('./pages/ops/backups'));
const ConfigPage = lazy(() => import('./pages/ops/config'));
const ChaosPage = lazy(() => import('./pages/ops/chaos'));
const QuotaPage = lazy(() => import('./pages/ops/quota'));
const GrafanaPage = lazy(() => import('./pages/ops/grafana'));

const menuData = [
  { path: '/dashboard', name: '总览', icon: <DashboardOutlined /> },
  { path: '/k8s', name: 'Kubernetes', icon: <CloudServerOutlined />, children: [
    { path: '/k8s/clusters', name: '集群管理' }, { path: '/k8s/pods', name: 'Pods' },
    { path: '/k8s/deployments', name: 'Deployments' }, { path: '/k8s/nodes', name: 'Nodes' },
    { path: '/k8s/terminal', name: 'Web Terminal' },
  ]},
  { path: '/cmdb', name: 'CMDB 资产', icon: <DatabaseOutlined /> },
  { path: '/ops', name: '运维中心', icon: <SettingOutlined />, children: [
    { path: '/ops/audit', name: '审计中心' }, { path: '/ops/events', name: '领域事件' },
    { path: '/ops/apikeys', name: 'API Keys' },
    { path: '/ops/users', name: '用户管理' }, { path: '/ops/scripts', name: '脚本库' },
    { path: '/ops/alerts', name: '告警中心' }, { path: '/ops/alert-rules', name: '告警规则' }, { path: '/ops/metrics', name: '指标看板' },
    { path: '/ops/logs', name: '日志检索' }, { path: '/ops/recordings', name: '录制回放' },
    { path: '/ops/approvals', name: '审批中心' }, { path: '/ops/secrets', name: '保险库' },
    { path: '/ops/oncall', name: '值班排班' }, { path: '/ops/traces', name: '链路追踪' },
    { path: '/ops/tickets', name: '工单系统' }, { path: '/ops/releases', name: '发布流水线' },
    { path: '/ops/runbooks', name: 'Runbook 剧本' }, { path: '/ops/chaos', name: '混沌演练' },
    { path: '/ops/capacity', name: '容量/成本' },
    { path: '/ops/backups', name: 'DB 备份状态' },
    { path: '/ops/config', name: '配置中心' },
    { path: '/ops/files', name: '文件管理' },
    { path: '/ops/quota', name: '资源配额' }, { path: '/ops/grafana', name: 'Grafana' },
  ]},
];

export default function App() {
  const nav = useNavigate(); const loc = useLocation();
  if (!useAuthStore((s) => s.isAuthenticated)) return <LoginPage />;
  return (
    <ProLayout title="SuperOps" logo={<SuperPet size={28} />} location={loc} menuDataRender={() => menuData}
      onMenuHeaderClick={() => nav('/dashboard')}
      menuItemRender={(item, dom) => <Link to={item.path || '/'}>{dom}</Link>}>
      <PageContainer>
        <Suspense fallback={<div style={{ padding: 48, textAlign: 'center', color: '#94a3b8' }}>加载中…</div>}>
        <Routes>
          <Route path="/dashboard" element={<Dashboard />} />
          <Route path="/k8s/clusters" element={<ClustersPage />} />
          <Route path="/k8s/clusters/:id" element={<ClusterDetail />} />
          <Route path="/k8s/pods" element={<PodsPage />} />
          <Route path="/k8s/deployments" element={<DeploymentsPage />} />
          <Route path="/k8s/nodes" element={<NodesPage />} />
          <Route path="/k8s/terminal" element={<TerminalPage />} />
          <Route path="/cmdb" element={<CmdbPage />} />
          <Route path="/ops/audit" element={<AuditPage />} />
          <Route path="/ops/events" element={<EventsPage />} />
          <Route path="/ops/apikeys" element={<ApiKeysPage />} />
          <Route path="/ops/users" element={<UsersPage />} />
          <Route path="/ops/scripts" element={<ScriptsPage />} />
          <Route path="/ops/alerts" element={<AlertsPage />} />
          <Route path="/ops/alert-rules" element={<AlertRulesPage />} />
          <Route path="/ops/metrics" element={<MetricsPage />} />
          <Route path="/ops/logs" element={<LogsPage />} />
          <Route path="/ops/recordings" element={<RecordingsPage />} />
          <Route path="/ops/approvals" element={<ApprovalsPage />} />
          <Route path="/ops/secrets" element={<SecretsPage />} />
          <Route path="/ops/oncall" element={<OncallPage />} />
          <Route path="/ops/traces" element={<TracesPage />} />
          <Route path="/ops/tickets" element={<TicketsPage />} />
          <Route path="/ops/releases" element={<ReleasesPage />} />
          <Route path="/ops/runbooks" element={<RunbooksPage />} />
          <Route path="/ops/chaos" element={<ChaosPage />} />
          <Route path="/ops/capacity" element={<CapacityPage />} />
          <Route path="/ops/backups" element={<BackupsPage />} />
          <Route path="/ops/config" element={<ConfigPage />} />
          <Route path="/ops/files" element={<FilesPage />} />
          <Route path="/ops/quota" element={<QuotaPage />} />
          <Route path="/ops/grafana" element={<GrafanaPage />} />
        </Routes>
        </Suspense>
      </PageContainer>
    </ProLayout>
  );
}
