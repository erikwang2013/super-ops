import { ProTable } from '@ant-design/pro-components';
import { useQuery } from '@tanstack/react-query';
import { k8sApi, Deployment } from '../../services/k8s';

export default function DeploymentsPage({ clusterId }: { clusterId: string }) {
  const { data, isLoading } = useQuery({ queryKey: ['deployments', clusterId], queryFn: () => k8sApi.listDeployments(clusterId) });
  return <ProTable<Deployment> columns={[
    { title: '名称', dataIndex: 'name' }, { title: '命名空间', dataIndex: 'namespace' },
    { title: '副本', dataIndex: 'replicas' }, { title: '就绪', dataIndex: 'ready_replicas' },
    { title: '运行时间', dataIndex: 'age' },
  ]} dataSource={data?.deployments || []} loading={isLoading} rowKey="name" search={false} headerTitle="Deployments" />;
}
