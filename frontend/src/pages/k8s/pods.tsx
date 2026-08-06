import { ProTable } from '@ant-design/pro-components';
import { useQuery } from '@tanstack/react-query';
import { useState } from 'react';
import { k8sApi, Pod } from '../../services/k8s';
import { ClusterPicker, useClusterPicker } from './cluster-select';

export default function PodsPage({ clusterId: fixed }: { clusterId?: string }) {
  const { defaultCluster } = useClusterPicker();
  const [picked, setPicked] = useState('');
  const clusterId = fixed ?? (picked || defaultCluster || 'default');
  const { data, isLoading } = useQuery({ queryKey: ['pods', clusterId], queryFn: () => k8sApi.listPods(clusterId) });
  return <>
    {!fixed && <ClusterPicker value={picked} onChange={setPicked} />}
    <ProTable<Pod> columns={[
      { title: '名称', dataIndex: 'name' }, { title: '命名空间', dataIndex: 'namespace' },
      { title: '状态', dataIndex: 'status' }, { title: '节点', dataIndex: 'node' },
      { title: '重启', dataIndex: 'restarts' }, { title: '运行时间', dataIndex: 'age' },
    ]} dataSource={data?.pods || []} loading={isLoading} rowKey="name" search={false} headerTitle={`Pods（集群 ${clusterId}）`} />
  </>;
}
