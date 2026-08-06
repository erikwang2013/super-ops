import { ProTable } from '@ant-design/pro-components';
import { useQuery } from '@tanstack/react-query';
import { useState } from 'react';
import { k8sApi, Deployment } from '../../services/k8s';
import { ClusterPicker, useClusterPicker } from './cluster-select';

export default function DeploymentsPage({ clusterId: fixed }: { clusterId?: string }) {
  const { defaultCluster } = useClusterPicker();
  const [picked, setPicked] = useState('');
  const clusterId = fixed ?? (picked || defaultCluster || 'default');
  const { data, isLoading } = useQuery({ queryKey: ['deployments', clusterId], queryFn: () => k8sApi.listDeployments(clusterId) });
  return <>
    {!fixed && <ClusterPicker value={picked} onChange={setPicked} />}
    <ProTable<Deployment> columns={[
      { title: '名称', dataIndex: 'name' }, { title: '命名空间', dataIndex: 'namespace' },
      { title: '副本', dataIndex: 'replicas' }, { title: '就绪', dataIndex: 'ready_replicas' },
      { title: '运行时间', dataIndex: 'age' },
    ]} dataSource={data?.deployments || []} loading={isLoading} rowKey="name" search={false} headerTitle={`Deployments（集群 ${clusterId}）`} />
  </>;
}
