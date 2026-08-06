import { ProTable } from '@ant-design/pro-components';
import { Alert, Button, Form, Input, Modal, Popconfirm, Space, Typography, message } from 'antd';
import { PlusOutlined, EyeOutlined, CopyOutlined, DeleteOutlined } from '@ant-design/icons';
import { useState } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { secretApi, SecretRow } from '../../services/api';

export default function SecretsPage() {
  const qc = useQueryClient();
  const [createOpen, setCreateOpen] = useState(false);
  const [viewing, setViewing] = useState<SecretRow | null>(null);
  const [value, setValue] = useState('');
  const [valueLoading, setValueLoading] = useState(false);
  const [form] = Form.useForm();

  const { data, isLoading, error } = useQuery({
    queryKey: ['secrets'],
    queryFn: () => secretApi.listSecrets(),
  });

  const invalidate = () => qc.invalidateQueries({ queryKey: ['secrets'] });

  const create = useMutation({
    mutationFn: (v: { name: string; value: string }) => secretApi.createSecret(v.name, v.value),
    onSuccess: () => { invalidate(); message.success('已保存'); setCreateOpen(false); form.resetFields(); },
    onError: (e: Error) => message.error(e.message),
  });

  const remove = useMutation({
    mutationFn: (name: string) => secretApi.deleteSecret(name),
    onSuccess: () => { invalidate(); message.success('已删除'); },
    onError: (e: Error) => message.error(e.message),
  });

  const openView = async (r: SecretRow) => {
    setViewing(r); setValue(''); setValueLoading(true);
    try {
      const res = await secretApi.getSecret(r.name);
      setValue(res.value);
    } catch (e) {
      message.error((e as Error).message);
    } finally {
      setValueLoading(false);
    }
  };

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(value);
      message.success('已复制到剪贴板');
    } catch {
      message.warning('复制失败，请手动选择文本');
    }
  };

  const columns = [
    { title: '名称', dataIndex: 'name' },
    { title: '创建时间', dataIndex: 'created_at', width: 180 },
    { title: '操作', width: 160, render: (_: unknown, r: SecretRow) => (
      <>
        <Button type="link" size="small" icon={<EyeOutlined />} onClick={() => openView(r)}>查看</Button>
        <Popconfirm title={`确定删除 secret "${r.name}"？`} onConfirm={() => remove.mutate(r.name)}>
          <Button type="link" danger size="small" icon={<DeleteOutlined />}>删除</Button>
        </Popconfirm>
      </>
    )},
  ];

  return <>
    {error && <Alert type="warning" showIcon style={{ marginBottom: 12 }}
      message="保险库不可用" description={error.message} />}
    <ProTable<SecretRow> rowKey="name" search={false} loading={isLoading}
      dataSource={data || []}
      columns={columns as never}
      headerTitle="凭据保险库（AES-256-GCM 加密存储，值不出现在列表）"
      toolBarRender={() => [
        <Button key="add" type="primary" icon={<PlusOutlined />} onClick={() => setCreateOpen(true)}>新建 Secret</Button>,
      ]}
      locale={{ emptyText: '暂无 Secret' }}
      options={false} />
    <Modal title="新建 Secret" open={createOpen}
      onCancel={() => { setCreateOpen(false); form.resetFields(); }}
      onOk={() => form.submit()} confirmLoading={create.isPending}>
      <Form form={form} onFinish={(v) => create.mutate(v)} layout="vertical">
        <Form.Item name="name" label="名称" rules={[
          { required: true, message: '请输入名称' },
          { pattern: /^[A-Za-z0-9._-]{1,128}$/, message: '仅限字母/数字/._-，最长 128 字符' },
        ]}>
          <Input placeholder="例如: db.password" />
        </Form.Item>
        <Form.Item name="value" label="值" rules={[
          { required: true, message: '请输入值' },
          { max: 65536, message: '最长 64KB' },
        ]}>
          <Input.TextArea rows={4} placeholder="明文值将加密后存储" />
        </Form.Item>
      </Form>
    </Modal>
    <Modal title={`Secret: ${viewing?.name || ''}`} open={!!viewing} footer={null} width={520}
      onCancel={() => setViewing(null)}>
      <Space direction="vertical" style={{ width: '100%' }}>
        <Typography.Text type="secondary">解密后的值（密文不会显示）：</Typography.Text>
        <Input.TextArea rows={5} value={valueLoading ? '加载中…' : value} readOnly
          style={{ fontFamily: 'monospace' }} />
        <Button icon={<CopyOutlined />} onClick={copy} disabled={valueLoading || !value}>复制到剪贴板</Button>
      </Space>
    </Modal>
  </>;
}
