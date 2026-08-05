import { useParams } from 'react-router-dom';
import { Card, Tabs } from 'antd';
import PodsPage from './pods';
import DeploymentsPage from './deployments';
import NodesPage from './nodes';

export default function ClusterDetail() {
  const { id } = useParams<{ id: string }>();
  return <Card title={`集群: ${id}`}><Tabs items={[
    { key: 'pods', label: 'Pods', children: <PodsPage /> },
    { key: 'deployments', label: 'Deployments', children: <DeploymentsPage /> },
    { key: 'nodes', label: 'Nodes', children: <NodesPage /> },
  ]} /></Card>;
}
