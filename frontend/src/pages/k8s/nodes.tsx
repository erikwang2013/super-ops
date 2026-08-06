import { ProTable } from '@ant-design/pro-components';
import { useQuery } from '@tanstack/react-query';
import { useState } from 'react';
import { k8sApi, NodeInfo } from '../../services/k8s';
import { ClusterPicker, useClusterPicker } from './cluster-select';

export default function NodesPage({ clusterId: fixed }: { clusterId?: string }) {
  const { defaultCluster } = useClusterPicker();
  const [picked, setPicked] = useState('');
  const clusterId = fixed ?? (picked || defaultCluster || 'default');
  const { data, isLoading } = useQuery({ queryKey: ['nodes', clusterId], queryFn: () => k8sApi.listNodes(clusterId) });
  return <>
    {!fixed && <ClusterPicker value={picked} onChange={setPicked} />}
    <ProTable<NodeInfo> columns={[
      { title: '名称', dataIndex: 'name' }, { title: '状态', dataIndex: 'status' },
      { title: '角色', dataIndex: 'role' }, { title: '版本', dataIndex: 'version' },
      { title: 'CPU', dataIndex: 'cpu' }, { title: '内存', dataIndex: 'memory' },
    ]} dataSource={data?.nodes || []} loading={isLoading} rowKey="name" search={false} headerTitle={`Nodes（集群 ${clusterId}）`} />
  </>;
}
