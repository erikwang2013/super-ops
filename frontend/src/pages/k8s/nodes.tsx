import { ProTable } from '@ant-design/pro-components';
import { useQuery } from '@tanstack/react-query';
import { k8sApi, NodeInfo } from '../../services/k8s';

export default function NodesPage() {
  const { data, isLoading } = useQuery({ queryKey: ['nodes'], queryFn: () => k8sApi.listNodes('default') });
  return <ProTable<NodeInfo> columns={[
    { title: '名称', dataIndex: 'name' }, { title: '状态', dataIndex: 'status' },
    { title: '角色', dataIndex: 'role' }, { title: '版本', dataIndex: 'version' },
    { title: 'CPU', dataIndex: 'cpu' }, { title: '内存', dataIndex: 'memory' },
  ]} dataSource={data?.nodes || []} loading={isLoading} rowKey="name" search={false} headerTitle="Nodes" />;
}
