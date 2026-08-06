import { ProTable } from '@ant-design/pro-components';
import { Button, Modal, Form, Input, Select, Popconfirm, Tag, message } from 'antd';
import { PlusOutlined } from '@ant-design/icons';
import { useState } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { cmdbApi, AssetRow, CmdbAssetInput } from '../../services/api';

const ASSET_TYPES = [
  { label: '主机', value: 'host' },
  { label: '交换机', value: 'switch' },
  { label: '路由器', value: 'router' },
  { label: '应用', value: 'app' },
  { label: '数据库', value: 'db' },
  { label: '存储', value: 'storage' },
];

const TYPE_COLORS: Record<string, string> = {
  host: 'green', switch: 'blue', router: 'purple', app: 'orange', db: 'gold', storage: 'cyan',
};

export default function CmdbPage() {
  const qc = useQueryClient();
  const [open, setOpen] = useState(false);
  const [form] = Form.useForm<CmdbAssetInput>();
  const { data, isLoading } = useQuery({ queryKey: ['cmdb-assets'], queryFn: () => cmdbApi.listAssets() });

  const create = useMutation({
    mutationFn: (v: CmdbAssetInput) => cmdbApi.createAsset(v),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['cmdb-assets'] });
      message.success('创建成功');
      setOpen(false);
      form.resetFields();
    },
    onError: (e: Error) => message.error(e.message),
  });
  const remove = useMutation({
    mutationFn: (id: number) => cmdbApi.deleteAsset(id),
    onSuccess: () => { qc.invalidateQueries({ queryKey: ['cmdb-assets'] }); message.success('已删除'); },
    onError: (e: Error) => message.error(e.message),
  });

  return <>
    <ProTable<AssetRow> rowKey="id" search={false} loading={isLoading} dataSource={data?.assets || []}
      headerTitle="CMDB 资产" pagination={{ pageSize: 20 }}
      toolBarRender={() => [<Button key="add" type="primary" icon={<PlusOutlined />} onClick={() => setOpen(true)}>新建资产</Button>]}
      columns={[
        { title: '类型', dataIndex: 'asset_type', render: (_, r) => <Tag color={TYPE_COLORS[r.asset_type] || 'default'}>{r.asset_type}</Tag> },
        { title: '名称', dataIndex: 'name' },
        { title: 'IP', dataIndex: 'ip', render: (_, r) => r.ip || '-' },
        { title: '环境', dataIndex: 'env' },
        { title: '负责人', dataIndex: 'owner' },
        { title: '状态', dataIndex: 'status', render: (_, r) => <Tag color={r.status === 'active' ? 'green' : 'red'}>{r.status}</Tag> },
        { title: '操作', width: 100, render: (_, r) => (
          <Popconfirm title="确认删除该资产？" onConfirm={() => remove.mutate(r.id)}>
            <Button type="link" danger size="small">删除</Button>
          </Popconfirm>
        )},
      ]} />
    <Modal title="新建资产" open={open}
      onCancel={() => { setOpen(false); form.resetFields(); }}
      onOk={() => form.submit()} confirmLoading={create.isPending}>
      <Form form={form} onFinish={(v) => create.mutate(v)} layout="vertical">
        <Form.Item name="asset_type" label="类型" rules={[{ required: true, message: '请选择类型' }]}>
          <Select options={ASSET_TYPES} placeholder="选择资产类型" />
        </Form.Item>
        <Form.Item name="name" label="名称" rules={[{ required: true, message: '请输入名称' }]}>
          <Input placeholder="资产名称" />
        </Form.Item>
        <Form.Item name="ip" label="IP">
          <Input placeholder="可选" />
        </Form.Item>
        <Form.Item name="env" label="环境">
          <Input placeholder="prod / staging / dev，可选" />
        </Form.Item>
        <Form.Item name="owner" label="负责人">
          <Input placeholder="可选" />
        </Form.Item>
        <Form.Item name="labels" label="Labels">
          <Input.TextArea placeholder="JSON，可选" rows={2} />
        </Form.Item>
      </Form>
    </Modal>
  </>;
}
