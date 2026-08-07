import { ProTable } from '@ant-design/pro-components';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { App, Button, Form, Input, Modal, Popconfirm, Select, Tag } from 'antd';
import { useState } from 'react';
import { ChaosRow, chaosApi } from '../../services/api';

const ACTIONS = ['restart', 'delete'];

const actionColor: Record<string, string> = { restart: 'blue', delete: 'red' };
const statusColor: Record<string, string> = {
  idle: 'default', running: 'processing', completed: 'success', failed: 'error',
};

export default function ChaosPage() {
  const { message } = App.useApp();
  const queryClient = useQueryClient();
  const [open, setOpen] = useState(false);
  const [form] = Form.useForm();

  const { data, isLoading } = useQuery({
    queryKey: ['chaos'],
    queryFn: () => chaosApi.listExperiments(),
  });

  const invalidate = () => queryClient.invalidateQueries({ queryKey: ['chaos'] });

  const create = useMutation({
    mutationFn: (v: any) => chaosApi.createExperiment(v),
    onSuccess: () => { message.success('演练已创建'); setOpen(false); form.resetFields(); invalidate(); },
    onError: (e: any) => message.error(e.message || '创建失败'),
  });

  const run = useMutation({
    mutationFn: (id: number) => chaosApi.runExperiment(id),
    onSuccess: (r: any) => {
      message.success(r.status === 'completed' ? '演练执行完成' : '演练执行失败');
      invalidate();
    },
    onError: (e: any) => { message.error(e.message || '执行失败'); invalidate(); },
  });

  const del = useMutation({
    mutationFn: (id: number) => chaosApi.deleteExperiment(id),
    onSuccess: () => { message.success('已删除'); invalidate(); },
    onError: (e: any) => message.error(e.message || '删除失败'),
  });

  return <>
    <ProTable<ChaosRow> rowKey="id" loading={isLoading} search={false}
      dataSource={data?.experiments || []} headerTitle="混沌演练（对 Deployment 执行重启/删除，验证自愈与监控）"
      toolBarRender={() => [<Button key="new" type="primary" onClick={() => setOpen(true)}>新建演练</Button>]}
      columns={[
        { title: '名称', dataIndex: 'name' },
        { title: '集群', dataIndex: 'cluster_id', width: 100 },
        { title: '目标', dataIndex: 'target_name', render: (_, r) =>
          <span><Tag color="purple">{r.target_type}</Tag> {r.target_name}</span> },
        { title: '动作', dataIndex: 'action', width: 90, render: (_, r) => <Tag color={actionColor[r.action]}>{r.action}</Tag> },
        { title: '状态', dataIndex: 'status', width: 100, render: (_, r) => <Tag color={statusColor[r.status]}>{r.status}</Tag> },
        { title: '操作人', dataIndex: 'operator', width: 90, render: (_, r) => r.operator || '-' },
        { title: '开始', dataIndex: 'started_at', width: 150, render: (_, r) => r.started_at || '-' },
        { title: '结束', dataIndex: 'ended_at', width: 150, render: (_, r) => r.ended_at || '-' },
        { title: '结果', dataIndex: 'error', ellipsis: true, render: (_, r) => r.error || '-' },
        {
          title: '操作', key: 'ops', width: 140, render: (_, r) => <>
            {r.status !== 'running' && (
              <Popconfirm title="确认执行该混沌动作？" onConfirm={() => run.mutate(r.id)}>
                <Button type="link" size="small">执行</Button>
              </Popconfirm>
            )}
            <Popconfirm title="确认删除该演练记录？" onConfirm={() => del.mutate(r.id)}>
              <Button type="link" danger size="small">删除</Button>
            </Popconfirm>
          </>,
        },
      ]} />
    <Modal title="新建混沌演练" open={open} onCancel={() => setOpen(false)}
      onOk={() => form.submit()} confirmLoading={create.isPending} destroyOnClose>
      <Form form={form} layout="vertical" onFinish={(v) => create.mutate(v)}>
        <Form.Item name="name" label="演练名称" rules={[{ required: true, max: 64 }]}>
          <Input placeholder="例如 重启 payment 服务" />
        </Form.Item>
        <Form.Item name="cluster_id" label="集群 ID" initialValue="default" rules={[{ required: true }]}>
          <Input placeholder="default" />
        </Form.Item>
        <Form.Item name="target_name" label="Deployment 名称" rules={[{ required: true, max: 128 }]}>
          <Input placeholder="例如 payment（namespace 固定 default）" />
        </Form.Item>
        <Form.Item name="action" label="动作" rules={[{ required: true }]}>
          <Select options={ACTIONS.map((a) => ({ label: a, value: a }))} placeholder="选择混沌动作" />
        </Form.Item>
        <Form.Item name="operator" label="操作人（可选）">
          <Input placeholder="默认空" />
        </Form.Item>
      </Form>
    </Modal>
  </>;
}
