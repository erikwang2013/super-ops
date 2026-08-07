import { ProTable } from '@ant-design/pro-components';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { App, Button, Form, Input, Modal, Select, Tag } from 'antd';
import { useState } from 'react';
import { ReleaseRow, releaseApi } from '../../services/api';

const STATUSES = ['pending', 'rolling', 'ok', 'failed'];

const statusColor: Record<string, string> = {
  pending: 'processing', rolling: 'warning', ok: 'success', failed: 'red',
};

export default function ReleasesPage() {
  const { message } = App.useApp();
  const queryClient = useQueryClient();
  const [open, setOpen] = useState(false);
  const [form] = Form.useForm();

  const { data, isLoading } = useQuery({
    queryKey: ['releases'],
    queryFn: () => releaseApi.listReleases(),
  });

  const invalidate = () => queryClient.invalidateQueries({ queryKey: ['releases'] });

  const create = useMutation({
    mutationFn: (v: any) => releaseApi.createRelease(v),
    onSuccess: (r: any) => {
      message.success(r.status === 'ok' ? '发布成功' : '发布失败，已记录');
      setOpen(false); form.resetFields(); invalidate();
    },
    onError: (e: any) => message.error(e.message || '发布失败'),
  });

  return <>
    <ProTable<ReleaseRow> rowKey="id" loading={isLoading} search={false}
      dataSource={data?.releases || []} headerTitle="发布流水线（更新 Deployment 镜像并记录）"
      toolBarRender={() => [<Button key="new" type="primary" onClick={() => setOpen(true)}>发起发布</Button>]}
      columns={[
        { title: '集群', dataIndex: 'cluster_id', width: 100 },
        { title: '命名空间', dataIndex: 'namespace', width: 120 },
        { title: 'Deployment', dataIndex: 'name' },
        {
          title: '镜像变更', key: 'images', render: (_, r) =>
            <span>{r.old_image === '-' ? '-' : r.old_image} <Tag color="blue">→</Tag> {r.new_image}</span>,
        },
        { title: '操作人', dataIndex: 'operator', render: (_, r) => r.operator || '-' },
        { title: '状态', dataIndex: 'status', render: (_, r) => <Tag color={statusColor[r.status]}>{r.status}</Tag> },
        { title: '时间', dataIndex: 'created_at', width: 160 },
      ]} />
    <Modal title="发起发布" open={open} onCancel={() => setOpen(false)}
      onOk={() => form.submit()} confirmLoading={create.isPending} destroyOnClose>
      <Form form={form} layout="vertical" onFinish={(v) => create.mutate(v)}>
        <Form.Item name="cluster_id" label="集群 ID" initialValue="default" rules={[{ required: true }]}>
          <Input placeholder="default" />
        </Form.Item>
        <Form.Item name="namespace" label="命名空间" rules={[{ required: true, max: 128 }]}>
          <Input placeholder="例如 prod" />
        </Form.Item>
        <Form.Item name="name" label="Deployment 名称" rules={[{ required: true, max: 128 }]}>
          <Input placeholder="例如 api-server" />
        </Form.Item>
        <Form.Item name="new_image" label="新镜像" rules={[{ required: true, max: 255 }]}>
          <Input placeholder="例如 nginx:1.27" />
        </Form.Item>
        <Form.Item name="old_image" label="旧镜像（可选）">
          <Input placeholder="留空则记录为 -" />
        </Form.Item>
        <Form.Item name="operator" label="操作人（可选）">
          <Input placeholder="默认空" />
        </Form.Item>
      </Form>
    </Modal>
  </>;
}
