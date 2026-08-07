import { ProTable } from '@ant-design/pro-components';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { App, Button, Form, Input, Modal, Select, Tag } from 'antd';
import { useState } from 'react';
import { TicketRow, ticketApi } from '../../services/api';

const SEVERITIES = ['LOW', 'MEDIUM', 'HIGH', 'CRIT'];
const STATUSES = ['open', 'assigned', 'resolved', 'closed'];

const severityColor: Record<string, string> = {
  LOW: 'blue', MEDIUM: 'orange', HIGH: 'red', CRIT: 'volcano',
};
const statusColor: Record<string, string> = {
  open: 'processing', assigned: 'warning', resolved: 'success', closed: 'default',
};

export default function TicketsPage() {
  const { message } = App.useApp();
  const queryClient = useQueryClient();
  const [open, setOpen] = useState(false);
  const [form] = Form.useForm();

  const { data, isLoading } = useQuery({
    queryKey: ['tickets'],
    queryFn: () => ticketApi.listTickets(),
  });

  const invalidate = () => queryClient.invalidateQueries({ queryKey: ['tickets'] });

  const create = useMutation({
    mutationFn: (v: any) => ticketApi.createTicket(v),
    onSuccess: () => { message.success('工单已创建'); setOpen(false); form.resetFields(); invalidate(); },
    onError: (e: any) => message.error(e.message || '创建失败'),
  });

  const setStatus = useMutation({
    mutationFn: ({ id, status }: { id: number; status: string }) => ticketApi.setStatus(id, status),
    onSuccess: () => { message.success('状态已更新'); invalidate(); },
    onError: (e: any) => message.error(e.message || '更新失败'),
  });

  const remove = useMutation({
    mutationFn: (id: number) => ticketApi.deleteTicket(id),
    onSuccess: () => { message.success('工单已删除'); invalidate(); },
    onError: (e: any) => message.error(e.message || '删除失败'),
  });

  return <>
    <ProTable<TicketRow> rowKey="id" loading={isLoading} search={false}
      dataSource={data?.tickets || []} headerTitle="工单（告警中心可一键建单）"
      toolBarRender={() => [<Button key="new" type="primary" onClick={() => setOpen(true)}>新建工单</Button>]}
      columns={[
        { title: '标题', dataIndex: 'title', ellipsis: true },
        { title: '级别', dataIndex: 'severity', render: (_, r) => <Tag color={severityColor[r.severity]}>{r.severity}</Tag> },
        { title: '状态', dataIndex: 'status', render: (_, r) => <Tag color={statusColor[r.status]}>{r.status}</Tag> },
        { title: '来源', dataIndex: 'source', render: (_, r) => r.source === 'alert' ? <Tag color="red">告警</Tag> : '手动' },
        { title: '负责人', dataIndex: 'assignee', render: (_, r) => r.assignee || '-' },
        { title: '创建时间', dataIndex: 'created_at' },
        { title: '操作', valueType: 'option', render: (_, r) => [
          r.status !== 'closed' &&
            <a key="close" onClick={() => setStatus.mutate({ id: r.id, status: 'closed' })}>关闭</a>,
          r.status === 'open' &&
            <a key="assign" onClick={() => setStatus.mutate({ id: r.id, status: 'assigned' })}>认领</a>,
          <a key="del" onClick={() => remove.mutate(r.id)}>删除</a>,
        ].filter(Boolean) },
      ]} />
    <Modal title="新建工单" open={open} onCancel={() => setOpen(false)}
      onOk={() => form.submit()} confirmLoading={create.isPending} destroyOnClose>
      <Form form={form} layout="vertical" onFinish={(v) => create.mutate(v)}>
        <Form.Item name="title" label="标题" rules={[{ required: true, max: 128 }]}>
          <Input placeholder="问题摘要" />
        </Form.Item>
        <Form.Item name="severity" label="级别" initialValue="LOW" rules={[{ required: true }]}>
          <Select options={SEVERITIES.map(v => ({ value: v, label: v }))} />
        </Form.Item>
        <Form.Item name="description" label="描述">
          <Input.TextArea rows={3} placeholder="详细说明（可选）" />
        </Form.Item>
      </Form>
    </Modal>
  </>;
}
