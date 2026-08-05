import { useParams } from 'react-router-dom';
import { Card, Tabs } from 'antd';
import PodsPage from './pods';
import DeploymentsPage from './deployments';
import NodesPage from './nodes';

export default function ClusterDetail() {
  const { id } = useParams<{ id: string }>();
  const clusterId = id ?? 'default';
  return <Card title={`集群: ${clusterId}`}><Tabs items={[
    { key: 'pods', label: 'Pods', children: <PodsPage clusterId={clusterId} /> },
    { key: 'deployments', label: 'Deployments', children: <DeploymentsPage clusterId={clusterId} /> },
    { key: 'nodes', label: 'Nodes', children: <NodesPage clusterId={clusterId} /> },
  ]} /></Card>;
}
