import { ProTable } from '@ant-design/pro-components';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { App, Button, Form, Input, InputNumber, Modal, Select, Space, Tabs, Tag, Typography } from 'antd';
import { useState } from 'react';
import { RunbookRow, RunbookRunRow, RunbookStep, runbookApi, scriptApi } from '../../services/api';

const { Text } = Typography;

const runStatusColor: Record<string, string> = {
  pending: 'default', running: 'processing', ok: 'success', failed: 'red',
};

function parseSteps(steps: string): RunbookStep[] {
  try { return JSON.parse(steps); } catch { return []; }
}

export default function RunbooksPage() {
  const { message } = App.useApp();
  const queryClient = useQueryClient();
  const [open, setOpen] = useState(false);
  const [runTarget, setRunTarget] = useState<RunbookRow | null>(null);
  const [form] = Form.useForm();
  const [runForm] = Form.useForm();

  const { data, isLoading } = useQuery({
    queryKey: ['runbooks'],
    queryFn: () => runbookApi.listRunbooks(),
  });
  const { data: runs } = useQuery({
    queryKey: ['runbook-runs'],
    queryFn: () => runbookApi.listRuns(),
  });
  const { data: scripts } = useQuery({
    queryKey: ['scripts'],
    queryFn: () => scriptApi.listScripts(),
  });
  const scriptOptions = (scripts?.scripts || []).map(s => ({ value: s.id, label: `${s.name}(#${s.id})` }));

  const invalidate = () => {
    queryClient.invalidateQueries({ queryKey: ['runbooks'] });
    queryClient.invalidateQueries({ queryKey: ['runbook-runs'] });
  };

  const create = useMutation({
    mutationFn: (v: { name: string; description?: string; steps: string }) => runbookApi.createRunbook(v),
    onSuccess: () => { message.success('剧本已创建'); setOpen(false); form.resetFields(); invalidate(); },
    onError: (e: any) => message.error(e.message || '创建失败'),
  });

  const remove = useMutation({
    mutationFn: (id: number) => runbookApi.deleteRunbook(id),
    onSuccess: () => { message.success('剧本已删除'); invalidate(); },
    onError: (e: any) => message.error(e.message || '删除失败'),
  });

  const run = useMutation({
    mutationFn: ({ id, body }: { id: number; body: any }) => runbookApi.runRunbook(id, body),
    onSuccess: () => { message.success('剧本执行完成'); setRunTarget(null); runForm.resetFields(); invalidate(); },
    onError: (e: any) => message.error(e.message || '执行失败'),
  });

  const runbookTable = (
    <ProTable<RunbookRow> rowKey="id" loading={isLoading} search={false}
      dataSource={data?.runbooks || []} headerTitle="剧本（按序执行脚本步骤，任一步失败即终止）"
      toolBarRender={() => [<Button key="new" type="primary" onClick={() => setOpen(true)}>新建剧本</Button>]}
      expandable={{
        expandedRowRender: (r) => (
          <Space direction="vertical" size={2}>
            {parseSteps(r.steps).map((s, i) => (
              <Text key={i} type="secondary">第 {i + 1} 步：{s.name}（脚本 #{s.script_id}，超时 {s.timeout_s}s）</Text>
            ))}
          </Space>
        ),
      }}
      columns={[
        { title: '名称', dataIndex: 'name' },
        { title: '步骤', dataIndex: 'steps', render: (_, r) => <Tag>{parseSteps(r.steps).length} 步</Tag> },
        { title: '描述', dataIndex: 'description', ellipsis: true, render: (_, r) => r.description || '-' },
        { title: '创建时间', dataIndex: 'created_at', width: 160 },
        { title: '操作', valueType: 'option', render: (_, r) => [
          <a key="run" onClick={() => setRunTarget(r)}>执行</a>,
          <a key="del" onClick={() => remove.mutate(r.id)}>删除</a>,
        ] },
      ]} />
  );

  const runsTable = (
    <ProTable<RunbookRunRow> rowKey="id" search={false}
      dataSource={runs?.runs || []} headerTitle="执行记录"
      expandable={{
        expandedRowRender: (r) => <pre style={{ whiteSpace: 'pre-wrap', fontSize: 12 }}>{r.output || '（无输出）'}</pre>,
      }}
      columns={[
        { title: '剧本', dataIndex: 'runbook_name' },
        { title: '目标 Pod', dataIndex: 'target_pods', ellipsis: true, render: (_, r) => r.target_pods || '-' },
        { title: '状态', dataIndex: 'status', render: (_, r) => <Tag color={runStatusColor[r.status]}>{r.status}</Tag> },
        { title: '开始', dataIndex: 'started_at', width: 160 },
        { title: '结束', dataIndex: 'finished_at', width: 160, render: (_, r) => r.finished_at || '-' },
      ]} />
  );

  return <>
    <Tabs defaultActiveKey="runbooks" items={[
      { key: 'runbooks', label: '剧本', children: runbookTable },
      { key: 'runs', label: '执行记录', children: runsTable },
    ]} />
    <Modal title="新建剧本" open={open} onCancel={() => setOpen(false)}
      onOk={() => form.submit()} confirmLoading={create.isPending} destroyOnClose>
      <Form form={form} layout="vertical" onFinish={(v) => {
        const steps = JSON.stringify((v.steps || []).map((s: any) => ({
          name: s.name, script_id: s.script_id, timeout_s: s.timeout_s || 300,
        })));
        create.mutate({ name: v.name, description: v.description, steps });
      }}>
        <Form.Item name="name" label="名称" rules={[{ required: true, max: 128 }]}>
          <Input placeholder="例如 发布前检查" />
        </Form.Item>
        <Form.Item name="description" label="描述">
          <Input placeholder="可选" />
        </Form.Item>
        <Form.List name="steps" rules={[{ validator: async (_, v) => { if (!v || !v.length) throw new Error('至少 1 步'); } }]}>
          {(fields, { add, remove }) => (
            <>
              {fields.map((f) => (
                <Space key={f.key} align="baseline" style={{ display: 'flex', marginBottom: 8 }}>
                  <Form.Item name={[f.name, 'script_id']} rules={[{ required: true, message: '选择脚本' }]}>
                    <Select style={{ width: 200 }} placeholder="脚本" options={scriptOptions} />
                  </Form.Item>
                  <Form.Item name={[f.name, 'name']} rules={[{ required: true, max: 128 }]}>
                    <Input placeholder="步骤名" style={{ width: 140 }} />
                  </Form.Item>
                  <Form.Item name={[f.name, 'timeout_s']} initialValue={300}>
                    <InputNumber min={1} max={3600} placeholder="超时s" style={{ width: 100 }} />
                  </Form.Item>
                  <a onClick={() => remove(f.name)}>删除</a>
                </Space>
              ))}
              <Button type="dashed" onClick={() => add()} block>添加步骤</Button>
            </>
          )}
        </Form.List>
      </Form>
    </Modal>
    <Modal title={`执行剧本：${runTarget?.name || ''}`} open={!!runTarget} onCancel={() => setRunTarget(null)}
      onOk={() => runForm.submit()} confirmLoading={run.isPending} destroyOnClose>
      <Form form={runForm} layout="vertical" onFinish={(v) => {
        const pods = (v.target_pods || '').split(/[\n,]/).map((p: string) => p.trim()).filter(Boolean);
        run.mutate({ id: runTarget!.id, body: { cluster_id: v.cluster_id, namespace: v.namespace, target_pods: pods } });
      }}>
        <Form.Item name="cluster_id" label="集群 ID" initialValue="default" rules={[{ required: true }]}>
          <Input />
        </Form.Item>
        <Form.Item name="namespace" label="命名空间" rules={[{ required: true, max: 128 }]}>
          <Input placeholder="例如 prod" />
        </Form.Item>
        <Form.Item name="target_pods" label="目标 Pod（逗号或换行分隔）">
          <Input.TextArea rows={3} placeholder="pod-a,pod-b" />
        </Form.Item>
      </Form>
    </Modal>
  </>;
}
