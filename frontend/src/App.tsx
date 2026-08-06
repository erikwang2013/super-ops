import { ProLayout, PageContainer } from '@ant-design/pro-components';
import { Routes, Route, useNavigate, useLocation, Link } from 'react-router-dom';
import { DashboardOutlined, CloudServerOutlined, DatabaseOutlined, SettingOutlined } from '@ant-design/icons';
import { useAuthStore } from './stores/auth';
import LoginPage from './pages/login';
import Dashboard from './pages/dashboard';
import ClustersPage from './pages/k8s/clusters';
import ClusterDetail from './pages/k8s/cluster-detail';
import PodsPage from './pages/k8s/pods';
import DeploymentsPage from './pages/k8s/deployments';
import NodesPage from './pages/k8s/nodes';
import TerminalPage from './pages/k8s/terminal';
import AuditPage from './pages/ops/audit';
import ApiKeysPage from './pages/ops/apikeys';
import UsersPage from './pages/ops/users';
import CmdbPage from './pages/cmdb';
import ScriptsPage from './pages/ops/scripts';
import AlertsPage from './pages/ops/alerts';
import MetricsPage from './pages/ops/metrics';
import LogsPage from './pages/ops/logs';
import RecordingsPage from './pages/ops/recordings';
import ApprovalsPage from './pages/ops/approvals';
import SecretsPage from './pages/ops/secrets';
import FilesPage from './pages/ops/files';

const menuData = [
  { path: '/dashboard', name: '总览', icon: <DashboardOutlined /> },
  { path: '/k8s', name: 'Kubernetes', icon: <CloudServerOutlined />, children: [
    { path: '/k8s/clusters', name: '集群管理' }, { path: '/k8s/pods', name: 'Pods' },
    { path: '/k8s/deployments', name: 'Deployments' }, { path: '/k8s/nodes', name: 'Nodes' },
    { path: '/k8s/terminal', name: 'Web Terminal' },
  ]},
  { path: '/cmdb', name: 'CMDB 资产', icon: <DatabaseOutlined /> },
  { path: '/ops', name: '运维中心', icon: <SettingOutlined />, children: [
    { path: '/ops/audit', name: '审计中心' }, { path: '/ops/apikeys', name: 'API Keys' },
    { path: '/ops/users', name: '用户管理' }, { path: '/ops/scripts', name: '脚本库' },
    { path: '/ops/alerts', name: '告警中心' }, { path: '/ops/metrics', name: '指标看板' },
    { path: '/ops/logs', name: '日志检索' }, { path: '/ops/recordings', name: '录制回放' },
    { path: '/ops/approvals', name: '审批中心' }, { path: '/ops/secrets', name: '保险库' },
    { path: '/ops/files', name: '文件管理' },
  ]},
];

export default function App() {
  const nav = useNavigate(); const loc = useLocation();
  if (!useAuthStore((s) => s.isAuthenticated)) return <LoginPage />;
  return (
    <ProLayout title="SuperOps" location={loc} menuDataRender={() => menuData}
      onMenuHeaderClick={() => nav('/dashboard')}
      menuItemRender={(item, dom) => <Link to={item.path || '/'}>{dom}</Link>}>
      <PageContainer>
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
          <Route path="/ops/apikeys" element={<ApiKeysPage />} />
          <Route path="/ops/users" element={<UsersPage />} />
          <Route path="/ops/scripts" element={<ScriptsPage />} />
          <Route path="/ops/alerts" element={<AlertsPage />} />
          <Route path="/ops/metrics" element={<MetricsPage />} />
          <Route path="/ops/logs" element={<LogsPage />} />
          <Route path="/ops/recordings" element={<RecordingsPage />} />
          <Route path="/ops/approvals" element={<ApprovalsPage />} />
          <Route path="/ops/secrets" element={<SecretsPage />} />
          <Route path="/ops/files" element={<FilesPage />} />
        </Routes>
      </PageContainer>
    </ProLayout>
  );
}
