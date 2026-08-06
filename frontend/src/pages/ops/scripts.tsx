import { ProColumns, ProTable } from '@ant-design/pro-components';
import { Button, Modal, Form, Input, InputNumber, Select, Tag, Popconfirm, message } from 'antd';
import { PlusOutlined, PlayCircleOutlined } from '@ant-design/icons';
import { useState } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { scriptApi, ScriptItem, ScriptRun } from '../../services/api';

const langColor: Record<string, string> = { shell: 'green', python: 'blue', go: 'cyan' };
const statusColor: Record<string, string> = { pending: 'orange', running: 'blue', succeeded: 'green', failed: 'red' };

export default function ScriptsPage() {
  const qc = useQueryClient();
  const [createOpen, setCreateOpen] = useState(false);
  const [runTarget, setRunTarget] = useState<ScriptItem | null>(null);
  const [output, setOutput] = useState<ScriptRun | null>(null);
  const [createForm] = Form.useForm();
  const [runForm] = Form.useForm();

  const { data, isLoading } = useQuery({ queryKey: ['scripts'], queryFn: () => scriptApi.listScripts() });
  const { data: runs, isLoading: runsLoading } = useQuery({ queryKey: ['scriptRuns'], queryFn: () => scriptApi.listRuns() });

  const invalidate = () => {
    qc.invalidateQueries({ queryKey: ['scripts'] });
    qc.invalidateQueries({ queryKey: ['scriptRuns'] });
  };

  const create = useMutation({
    mutationFn: (v: ScriptItem & { content: string }) => scriptApi.createScript({
      name: v.name, description: v.description || '', language: v.language, content: v.content, timeout_s: v.timeout_s,
    }),
    onSuccess: () => { invalidate(); message.success('已创建'); setCreateOpen(false); createForm.resetFields(); },
    onError: (e: Error) => message.error(e.message),
  });
  const remove = useMutation({
    mutationFn: (id: number) => scriptApi.deleteScript(id),
    onSuccess: () => { invalidate(); message.success('已删除'); },
    onError: (e: Error) => message.error(e.message),
  });
  const run = useMutation({
    mutationFn: (v: { id: number; cluster_id: string; namespace: string; target_pods: string }) => {
      const target_pods = v.target_pods.split(',').map(s => s.trim()).filter(Boolean);
      return scriptApi.runScript(v.id, { cluster_id: v.cluster_id, namespace: v.namespace, target_pods });
    },
    onSuccess: () => { invalidate(); message.success('已触发运行'); setRunTarget(null); runForm.resetFields(); },
    onError: (e: Error) => message.error(e.message),
  });

  const columns: ProColumns<ScriptItem>[] = [
    { title: 'ID', dataIndex: 'id', width: 70 },
    { title: '名称', dataIndex: 'name' },
    { title: '语言', dataIndex: 'language', width: 90, render: (v) => <Tag color={langColor[String(v)] || 'default'}>{String(v)}</Tag> },
    { title: '描述', dataIndex: 'description', ellipsis: true },
    { title: '超时(s)', dataIndex: 'timeout_s', width: 90 },
    { title: '创建时间', dataIndex: 'created_at', width: 160 },
    { title: '操作', width: 140, render: (_: unknown, r: ScriptItem) => (
      <>
        <Button type="link" size="small" icon={<PlayCircleOutlined />} onClick={() => { setRunTarget(r); runForm.setFieldsValue({ target_pods: '' }); }}>运行</Button>
        <Popconfirm title="确定删除该脚本？" onConfirm={() => remove.mutate(r.id)}>
          <Button type="link" danger size="small">删除</Button>
        </Popconfirm>
      </>
    )},
  ];

  return <>
    <ProTable<ScriptItem> rowKey="id" search={false} loading={isLoading} dataSource={data?.scripts || []}
      headerTitle="脚本库"
      toolBarRender={() => [<Button key="add" type="primary" icon={<PlusOutlined />} onClick={() => setCreateOpen(true)}>新建脚本</Button>]}
      columns={columns} />
    <ProTable<ScriptRun> rowKey="id" search={false} loading={runsLoading} dataSource={runs?.runs || []}
      headerTitle="运行记录（最近 50 条）"
      columns={[
        { title: 'ID', dataIndex: 'id', width: 70 },
        { title: '脚本', dataIndex: 'script_id', width: 90 },
        { title: '目标 Pods', dataIndex: 'target_pods', ellipsis: true },
        { title: '状态', dataIndex: 'status', width: 100, render: (v) => <Tag color={statusColor[String(v)] || 'default'}>{String(v)}</Tag> },
        { title: '开始时间', dataIndex: 'started_at', width: 160 },
        { title: '操作', width: 100, render: (_: unknown, r: ScriptRun) => (
          <Button type="link" size="small" onClick={() => setOutput(r)}>查看输出</Button>
        )},
      ]} />
    <Modal title="新建脚本" open={createOpen}
      onCancel={() => { setCreateOpen(false); createForm.resetFields(); }}
      onOk={() => createForm.submit()} confirmLoading={create.isPending}>
      <Form form={createForm} onFinish={(v) => create.mutate(v)} layout="vertical">
        <Form.Item name="name" label="名称" rules={[{ required: true, message: '请输入名称' }]}>
          <Input placeholder="例如: 清理旧日志" />
        </Form.Item>
        <Form.Item name="description" label="描述">
          <Input placeholder="可选" />
        </Form.Item>
        <Form.Item name="language" label="语言" initialValue="shell" rules={[{ required: true }]}>
          <Select options={[{ value: 'shell', label: 'shell' }, { value: 'python', label: 'python' }, { value: 'go', label: 'go' }]} />
        </Form.Item>
        <Form.Item name="content" label="脚本内容" rules={[{ required: true, message: '请输入脚本内容' }]}>
          <Input.TextArea rows={10} placeholder="set -e&#10;echo hello" />
        </Form.Item>
        <Form.Item name="timeout_s" label="超时(秒)" initialValue={300}>
          <InputNumber min={1} max={3600} style={{ width: '100%' }} />
        </Form.Item>
      </Form>
    </Modal>
    <Modal title={`运行脚本: ${runTarget?.name || ''}`} open={!!runTarget}
      onCancel={() => setRunTarget(null)}
      onOk={() => runForm.submit()} confirmLoading={run.isPending}>
      <Form form={runForm} onFinish={(v) => runTarget && run.mutate({ ...v, id: runTarget.id })} layout="vertical">
        <Form.Item name="cluster_id" label="集群 ID" rules={[{ required: true, message: '请输入集群 ID' }]}>
          <Input placeholder="default" />
        </Form.Item>
        <Form.Item name="namespace" label="命名空间" rules={[{ required: true, message: '请输入命名空间' }]}>
          <Input placeholder="default" />
        </Form.Item>
        <Form.Item name="target_pods" label="目标 Pods">
          <Input placeholder="逗号分隔的 Pod 名称，可留空" />
        </Form.Item>
      </Form>
    </Modal>
    <Modal title={`运行输出 #${output?.id || ''}`} open={!!output} footer={null} width={720}
      onCancel={() => setOutput(null)}>
      <pre style={{ whiteSpace: 'pre-wrap', maxHeight: 480, overflow: 'auto' }}>
        {output?.output || '（无输出）'}
      </pre>
    </Modal>
  </>;
}
