import { ProTable } from '@ant-design/pro-components';
import { Switch, message } from 'antd';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { opsApi, UserRow } from '../../services/api';

export default function UsersPage() {
  const qc = useQueryClient();
  const { data, isLoading } = useQuery({ queryKey: ['users'], queryFn: () => opsApi.listUsers() });

  const setStatus = useMutation({
    mutationFn: (v: { id: string; status: string }) => opsApi.setUserStatus(v.id, v.status),
    onSuccess: (_d, v) => {
      qc.invalidateQueries({ queryKey: ['users'] });
      message.success(`已将 ${v.status === 'enabled' ? '启用' : '禁用'} 该用户`);
    },
    onError: (e: Error) => message.error(e.message),
  });

  return <ProTable<UserRow> rowKey="id" search={false} loading={isLoading} dataSource={data?.users || []}
    headerTitle="用户管理" pagination={{ pageSize: 20 }}
    columns={[
      { title: '用户名', dataIndex: 'username' },
      { title: '邮箱', dataIndex: 'email', ellipsis: true },
      { title: '角色', dataIndex: 'role' },
      { title: '状态', dataIndex: 'status', render: (_, r) => (
        <Switch checked={r.status === 'enabled'}
          loading={setStatus.isPending && setStatus.variables?.id === r.id}
          onChange={(checked) => setStatus.mutate({ id: r.id, status: checked ? 'enabled' : 'disabled' })} />
      )},
      { title: '创建时间', dataIndex: 'created_at' },
    ]} />;
}
