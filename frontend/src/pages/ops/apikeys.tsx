import { ProTable } from '@ant-design/pro-components';
import { Button, Modal, Form, Input, Popconfirm, message, Alert } from 'antd';
import { PlusOutlined, CopyOutlined } from '@ant-design/icons';
import { useState } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { opsApi, ApiKeyRow, ApiKeyCreated } from '../../services/api';

export default function ApiKeysPage() {
  const qc = useQueryClient();
  const [open, setOpen] = useState(false);
  const [created, setCreated] = useState<ApiKeyCreated | null>(null);
  const [form] = Form.useForm();
  const { data, isLoading } = useQuery({ queryKey: ['apiKeys'], queryFn: () => opsApi.listApiKeys() });

  const create = useMutation({
    mutationFn: (v: { name: string }) => opsApi.createApiKey(v.name),
    onSuccess: (d) => { qc.invalidateQueries({ queryKey: ['apiKeys'] }); setCreated(d); form.resetFields(); },
    onError: (e: Error) => message.error(e.message),
  });
  const remove = useMutation({
    mutationFn: (id: string) => opsApi.deleteApiKey(id),
    onSuccess: () => { qc.invalidateQueries({ queryKey: ['apiKeys'] }); message.success('已吊销'); },
    onError: (e: Error) => message.error(e.message),
  });
  const copyKey = () => {
    if (!created) return;
    navigator.clipboard.writeText(created.key)
      .then(() => message.success('已复制'))
      .catch(() => message.error('复制失败'));
  };

  return <>
    <ProTable<ApiKeyRow> rowKey="id" search={false} loading={isLoading} dataSource={data?.keys || []}
      headerTitle="API Keys"
      toolBarRender={() => [<Button key="add" type="primary" icon={<PlusOutlined />} onClick={() => setOpen(true)}>新建</Button>]}
      columns={[
        { title: '名称', dataIndex: 'name' },
        { title: 'ID', dataIndex: 'id', ellipsis: true },
        { title: '操作', width: 100, render: (_, r) => (
          <Popconfirm title="确定吊销该 API Key？" onConfirm={() => remove.mutate(r.id)}>
            <Button type="link" danger size="small">吊销</Button>
          </Popconfirm>
        )},
      ]} />
    <Modal title="新建 API Key" open={open}
      onCancel={() => { setOpen(false); setCreated(null); }}
      onOk={() => form.submit()} confirmLoading={create.isPending}
      footer={created ? [<Button key="copy" type="primary" icon={<CopyOutlined />} onClick={copyKey}>复制 Key</Button>,
        <Button key="close" onClick={() => { setOpen(false); setCreated(null); }}>关闭</Button>] : undefined}>
      {created ? (
        <Alert type="warning" showIcon message={created.warning} description={<code>{created.key}</code>} />
      ) : (
        <Form form={form} onFinish={(v) => create.mutate(v)} layout="vertical">
          <Form.Item name="name" label="名称" rules={[{ required: true, message: '请输入名称' }]}>
            <Input placeholder="my-service-key" />
          </Form.Item>
        </Form>
      )}
    </Modal>
  </>;
}
