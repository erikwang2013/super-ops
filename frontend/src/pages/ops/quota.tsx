import { ProTable } from '@ant-design/pro-components';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { App, Button, Form, Input, InputNumber, Modal, Popconfirm, Tag } from 'antd';
import { useState } from 'react';
import { QuotaRow, quotaApi } from '../../services/api';

export default function QuotaPage() {
  const { message } = App.useApp();
  const queryClient = useQueryClient();
  const [open, setOpen] = useState(false);
  const [form] = Form.useForm();

  const { data, isLoading } = useQuery({
    queryKey: ['quota'],
    queryFn: () => quotaApi.listQuotas(),
  });

  const invalidate = () => queryClient.invalidateQueries({ queryKey: ['quota'] });

  const create = useMutation({
    mutationFn: (v: any) => quotaApi.createQuota(v),
    onSuccess: () => { message.success('配额已保存'); setOpen(false); form.resetFields(); invalidate(); },
    onError: (e: any) => message.error(e.message || '保存失败'),
  });

  const del = useMutation({
    mutationFn: (id: number) => quotaApi.deleteQuota(id),
    onSuccess: () => { message.success('已删除'); invalidate(); },
    onError: (e: any) => message.error(e.message || '删除失败'),
  });

  return <>
    <ProTable<QuotaRow> rowKey="id" loading={isLoading} search={false}
      dataSource={data?.quotas || []} headerTitle="资源配额（命名空间级 CPU/内存请求与限制登记）"
      toolBarRender={() => [<Button key="new" type="primary" onClick={() => setOpen(true)}>新建配额</Button>]}
      columns={[
        { title: '命名空间', dataIndex: 'namespace', render: (_, r) => <Tag color="blue">{r.namespace}</Tag> },
        { title: '集群', dataIndex: 'cluster_id', width: 100 },
        { title: 'CPU 请求', dataIndex: 'cpu_request', width: 100, render: (_, r) => r.cpu_request || '-' },
        { title: '内存请求', dataIndex: 'memory_request', width: 110, render: (_, r) => r.memory_request || '-' },
        { title: 'CPU 上限', dataIndex: 'cpu_limit', width: 100, render: (_, r) => r.cpu_limit || '-' },
        { title: '内存上限', dataIndex: 'memory_limit', width: 110, render: (_, r) => r.memory_limit || '-' },
        { title: '副本数', dataIndex: 'replicas', width: 80, render: (_, r) => r.replicas || 0 },
        { title: '说明', dataIndex: 'description', ellipsis: true, render: (_, r) => r.description || '-' },
        { title: '创建时间', dataIndex: 'created_at', width: 150 },
        {
          title: '操作', key: 'ops', width: 80, render: (_, r) => (
            <Popconfirm title="确认删除该配额？" onConfirm={() => del.mutate(r.id)}>
              <Button type="link" danger size="small">删除</Button>
            </Popconfirm>
          ),
        },
      ]} />
    <Modal title="新建/更新配额" open={open} onCancel={() => setOpen(false)}
      onOk={() => form.submit()} confirmLoading={create.isPending} destroyOnClose>
      <Form form={form} layout="vertical" onFinish={(v) => create.mutate(v)}>
        <Form.Item name="cluster_id" label="集群 ID" initialValue="default" rules={[{ required: true, max: 64 }]}>
          <Input placeholder="default" />
        </Form.Item>
        <Form.Item name="namespace" label="命名空间" rules={[{ required: true, max: 64 }]}>
          <Input placeholder="例如 web（cluster+namespace 重复时覆盖）" />
        </Form.Item>
        <Form.Item name="cpu_request" label="CPU 请求（如 500m）">
          <Input placeholder="500m" />
        </Form.Item>
        <Form.Item name="memory_request" label="内存请求（如 1Gi）">
          <Input placeholder="1Gi" />
        </Form.Item>
        <Form.Item name="cpu_limit" label="CPU 上限（如 2）">
          <Input placeholder="2" />
        </Form.Item>
        <Form.Item name="memory_limit" label="内存上限（如 4Gi）">
          <Input placeholder="4Gi" />
        </Form.Item>
        <Form.Item name="replicas" label="副本数">
          <InputNumber min={0} style={{ width: '100%' }} placeholder="0" />
        </Form.Item>
        <Form.Item name="description" label="说明（可选）">
          <Input placeholder="业务说明" />
        </Form.Item>
      </Form>
    </Modal>
  </>;
}
