import { ProLayout, PageContainer } from '@ant-design/pro-components';
import { Routes, Route, useNavigate, useLocation, Link } from 'react-router-dom';
import { DashboardOutlined, CloudServerOutlined } from '@ant-design/icons';
import { useAuthStore } from './stores/auth';
import LoginPage from './pages/login';
import Dashboard from './pages/dashboard';
import ClustersPage from './pages/k8s/clusters';
import ClusterDetail from './pages/k8s/cluster-detail';
import PodsPage from './pages/k8s/pods';
import DeploymentsPage from './pages/k8s/deployments';
import NodesPage from './pages/k8s/nodes';
import TerminalPage from './pages/k8s/terminal';

const menuData = [
  { path: '/dashboard', name: '总览', icon: <DashboardOutlined /> },
  { path: '/k8s', name: 'Kubernetes', icon: <CloudServerOutlined />, children: [
    { path: '/k8s/clusters', name: '集群管理' }, { path: '/k8s/pods', name: 'Pods' },
    { path: '/k8s/deployments', name: 'Deployments' }, { path: '/k8s/nodes', name: 'Nodes' },
    { path: '/k8s/terminal', name: 'Web Terminal' },
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
          <Route path="/k8s/pods" element={<PodsPage clusterId="default" />} />
          <Route path="/k8s/deployments" element={<DeploymentsPage clusterId="default" />} />
          <Route path="/k8s/nodes" element={<NodesPage clusterId="default" />} />
          <Route path="/k8s/terminal" element={<TerminalPage />} />
        </Routes>
      </PageContainer>
    </ProLayout>
  );
}
