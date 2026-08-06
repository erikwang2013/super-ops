import { ProTable } from '@ant-design/pro-components';
import { Button, Form, Input, Modal, Select, Space, Tag, Popconfirm, message } from 'antd';
import { PlusOutlined, CheckOutlined, CloseOutlined, StopOutlined, RedoOutlined } from '@ant-design/icons';
import { useState } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { approvalApi, ApprovalRow } from '../../services/api';

const kindColor: Record<string, string> = { delete: 'red', scale: 'orange', restart: 'blue', generic: 'default' };
const statusColor: Record<string, string> = { pending: 'orange', approved: 'green', rejected: 'red', canceled: 'default' };
const KIND_OPTIONS = ['delete', 'scale', 'restart', 'generic'];

export default function ApprovalsPage() {
  const qc = useQueryClient();
  const [status, setStatus] = useState<string>('');
  const [createOpen, setCreateOpen] = useState(false);
  const [form] = Form.useForm();

  const { data, isLoading, error } = useQuery({
    queryKey: ['approvals', status],
    queryFn: () => approvalApi.listApprovals(status || undefined),
  });

  const invalidate = () => qc.invalidateQueries({ queryKey: ['approvals'] });

  const create = useMutation({
    mutationFn: (v: { kind: string; target: string; reason?: string }) => approvalApi.createApproval(v),
    onSuccess: () => { invalidate(); message.success('已提交审批单'); setCreateOpen(false); form.resetFields(); },
    onError: (e: Error) => message.error(e.message),
  });

  const decide = useMutation({
    mutationFn: (v: { id: number; action: string }) => approvalApi.decideApproval(v.id, v.action),
    onSuccess: () => { invalidate(); message.success('已处理'); },
    onError: (e: Error) => message.error(e.message),
  });

  const columns = [
    { title: 'ID', dataIndex: 'id', width: 70 },
    { title: '类型', dataIndex: 'kind', width: 90, render: (v: unknown, r: ApprovalRow) =>
      <Tag color={kindColor[r.kind] || 'default'}>{String(v)}</Tag> },
    { title: '目标', dataIndex: 'target', ellipsis: true },
    { title: '申请人', dataIndex: 'operator', width: 110 },
    { title: '理由', dataIndex: 'reason', ellipsis: true },
    { title: '状态', dataIndex: 'status', width: 90, render: (v: unknown, r: ApprovalRow) =>
      <Tag color={statusColor[r.status] || 'default'}>{String(v)}</Tag> },
    { title: '提交时间', dataIndex: 'created_at', width: 160 },
    { title: '审批人', dataIndex: 'decided_by', width: 110, render: (v: unknown) => String(v || '-') },
    { title: '操作', width: 220, render: (_: unknown, r: ApprovalRow) => {
      const actions: { label: string; action: string; icon?: React.ReactNode; danger?: boolean }[] = [];
      if (r.status === 'pending') {
        actions.push({ label: '通过', action: 'approve', icon: <CheckOutlined /> });
        actions.push({ label: '拒绝', action: 'reject', icon: <CloseOutlined />, danger: true });
        actions.push({ label: '取消', action: 'cancel', icon: <StopOutlined /> });
      } else if (r.status === 'rejected') {
        actions.push({ label: '重新打开', action: 'reopen', icon: <RedoOutlined /> });
      }
      return actions.length ? (
        <Space size={0}>
          {actions.map((a) => (
            <Popconfirm key={a.action} title={`确定${a.label}该审批单？`}
              onConfirm={() => decide.mutate({ id: r.id, action: a.action })}>
              <Button type="link" size="small" danger={a.danger} icon={a.icon}>{a.label}</Button>
            </Popconfirm>
          ))}
        </Space>
      ) : <span style={{ color: '#999' }}>—</span>;
    }},
  ];

  return <>
    <ProTable<ApprovalRow> rowKey="id" search={false} loading={isLoading}
      dataSource={data?.approvals || []}
      columns={columns as never}
      headerTitle="审批中心"
      toolBarRender={() => [
        <Select key="status" allowClear placeholder="按状态过滤" style={{ width: 140 }}
          value={status || undefined}
          onChange={(v) => setStatus(v || '')}
          options={['pending', 'approved', 'rejected', 'canceled'].map((s) => ({ value: s, label: s }))} />,
        <Button key="add" type="primary" icon={<PlusOutlined />} onClick={() => setCreateOpen(true)}>新建审批单</Button>,
      ]}
      locale={{ emptyText: error ? `查询失败：${error.message}` : '暂无审批单' }}
      options={false} />
    <Modal title="新建审批单" open={createOpen}
      onCancel={() => { setCreateOpen(false); form.resetFields(); }}
      onOk={() => form.submit()} confirmLoading={create.isPending}>
      <Form form={form} onFinish={(v) => create.mutate(v)} layout="vertical">
        <Form.Item name="kind" label="类型" initialValue="delete" rules={[{ required: true }]}>
          <Select options={KIND_OPTIONS.map((k) => ({ value: k, label: k }))} />
        </Form.Item>
        <Form.Item name="target" label="目标" rules={[{ required: true, message: '请输入目标' }]}
          extra="delete 类型约定格式: {cluster_id}/{namespace}/{name}">
          <Input placeholder="default/default/my-app" />
        </Form.Item>
        <Form.Item name="reason" label="理由" rules={[{ max: 512, message: '最长 512 字' }]}>
          <Input.TextArea rows={3} placeholder="可选" />
        </Form.Item>
      </Form>
    </Modal>
  </>;
}
