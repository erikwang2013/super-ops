import { ProTable } from '@ant-design/pro-components';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { App, Button, Form, Input, Modal, Select, Switch } from 'antd';
import { useState } from 'react';
import { AlertRuleRow, alertRuleApi } from '../../services/api';

const METRICS: { value: string; label: string }[] = [
  { value: 'node_not_ready', label: '节点不可用（计数）' },
  { value: 'node_ready_pct', label: '节点就绪率（%）' },
  { value: 'pod_not_running', label: 'Pod 未运行（计数）' },
  { value: 'pod_running_pct', label: 'Pod 运行率（%）' },
  { value: 'deployment_unavailable', label: 'Deployment 副本不足（计数）' },
  { value: 'deployment_ready_pct', label: 'Deployment 就绪率（%）' },
];

const LEVELS = ['INFO', 'WARN', 'CRIT'];
const ACTIONS = ['notify', 'restart', 'scale'];

const levelColor: Record<string, string> = { INFO: 'blue', WARN: 'orange', CRIT: 'red' };

export default function AlertRulesPage() {
  const { message } = App.useApp();
  const queryClient = useQueryClient();
  const [open, setOpen] = useState(false);
  const [form] = Form.useForm();

  const { data, isLoading } = useQuery({
    queryKey: ['alert-rules'],
    queryFn: () => alertRuleApi.listRules(),
  });

  const invalidate = () => queryClient.invalidateQueries({ queryKey: ['alert-rules'] });

  const create = useMutation({
    mutationFn: (v: any) => alertRuleApi.createRule({ ...v, threshold: String(v.threshold) }),
    onSuccess: () => { message.success('规则已创建'); setOpen(false); form.resetFields(); invalidate(); },
    onError: (e: any) => message.error(e.message || '创建失败'),
  });

  const toggle = useMutation({
    mutationFn: ({ id, enabled }: { id: number; enabled: boolean }) => alertRuleApi.setEnabled(id, enabled),
    onSuccess: invalidate,
    onError: (e: any) => message.error(e.message || '更新失败'),
  });

  const remove = useMutation({
    mutationFn: (id: number) => alertRuleApi.deleteRule(id),
    onSuccess: () => { message.success('规则已删除'); invalidate(); },
    onError: (e: any) => message.error(e.message || '删除失败'),
  });

  const confirmDelete = (row: AlertRuleRow) => {
    Modal.confirm({
      title: `删除规则「${row.name}」？`,
      content: '删除后 collector 不再按此规则告警（内置种子规则可重新建）。',
      onOk: () => remove.mutate(row.id),
    });
  };

  return <>
    <ProTable<AlertRuleRow> rowKey="id" loading={isLoading} search={false}
      dataSource={data?.rules || []} headerTitle="告警规则（collector 每轮巡检按启用的规则求值）"
      toolBarRender={() => [<Button key="new" type="primary" onClick={() => setOpen(true)}>新建规则</Button>]}
      columns={[
        { title: '名称', dataIndex: 'name' },
        { title: '指标', dataIndex: 'metric', render: (_: any, r) => METRICS.find(m => m.value === r.metric)?.label || r.metric },
        { title: '条件', dataIndex: 'operator', render: (_: any, r) => `${r.operator === 'ge' ? '≥' : '≤'} ${r.threshold}${r.metric.endsWith('_pct') ? '%' : ''}` },
        { title: '级别', dataIndex: 'level', render: (_, r) => <span style={{ color: levelColor[r.level] }}>{r.level}</span> },
        { title: '动作', dataIndex: 'action' },
        { title: '启用', dataIndex: 'enabled', render: (_, r) =>
          <Switch checked={r.enabled} loading={toggle.isPending} onChange={enabled => toggle.mutate({ id: r.id, enabled })} /> },
        { title: '操作', valueType: 'option', render: (_, r) => [
          <a key="del" onClick={() => confirmDelete(r)}>删除</a>,
        ] },
      ]} />
    <Modal title="新建告警规则" open={open} onCancel={() => setOpen(false)}
      onOk={() => form.submit()} confirmLoading={create.isPending} destroyOnClose>
      <Form form={form} layout="vertical" onFinish={(v) => create.mutate(v)}>
        <Form.Item name="name" label="规则名称" rules={[{ required: true, max: 128 }]}>
          <Input placeholder="如 node-ready-above-80" />
        </Form.Item>
        <Form.Item name="metric" label="指标" rules={[{ required: true }]}>
          <Select options={METRICS} />
        </Form.Item>
        <Form.Item name="operator" label="触发条件" initialValue="ge" rules={[{ required: true }]}>
          <Select options={[{ value: 'ge', label: '≥ 阈值（ge）' }, { value: 'le', label: '≤ 阈值（le）' }]} />
        </Form.Item>
        <Form.Item name="threshold" label="阈值" initialValue="1" rules={[{ required: true, pattern: /^\d+(\.\d+)?$/, message: '数字' }]}>
          <Input placeholder="计数类 1-1000；百分比类 0-100" />
        </Form.Item>
        <Form.Item name="level" label="级别" initialValue="WARN" rules={[{ required: true }]}>
          <Select options={LEVELS.map(v => ({ value: v, label: v }))} />
        </Form.Item>
        <Form.Item name="action" label="动作" initialValue="notify" rules={[{ required: true }]}>
          <Select options={ACTIONS.map(v => ({ value: v, label: v === 'notify' ? '仅通知' : v === 'restart' ? '自愈重启（需自愈开关开启）' : '自愈扩容（需自愈开关开启）' }))} />
        </Form.Item>
      </Form>
    </Modal>
  </>;
}
