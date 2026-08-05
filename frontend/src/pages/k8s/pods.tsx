import { ProTable } from '@ant-design/pro-components';
import { useQuery } from '@tanstack/react-query';
import { k8sApi, Pod } from '../../services/k8s';

export default function PodsPage({ clusterId }: { clusterId: string }) {
  const { data, isLoading } = useQuery({ queryKey: ['pods', clusterId], queryFn: () => k8sApi.listPods(clusterId) });
  return <ProTable<Pod> columns={[
    { title: '名称', dataIndex: 'name' }, { title: '命名空间', dataIndex: 'namespace' },
    { title: '状态', dataIndex: 'status' }, { title: '节点', dataIndex: 'node' },
    { title: '重启', dataIndex: 'restarts' }, { title: '运行时间', dataIndex: 'age' },
  ]} dataSource={data?.pods || []} loading={isLoading} rowKey="name" search={false} headerTitle="Pods" />;
}
