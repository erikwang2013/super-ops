import { ProTable } from '@ant-design/pro-components';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { App, Button, Input, Modal, Space, Tag } from 'antd';
import { useState } from 'react';
import { ConfigKeyRow, configRemoteApi } from '../../services/api';

export default function ConfigPage() {
  const qc = useQueryClient();
  const { message } = App.useApp();
  const [editing, setEditing] = useState<ConfigKeyRow | null>(null);
  const [value, setValue] = useState('');
  const { data, isLoading } = useQuery({
    queryKey: ['config-remote-keys'],
    queryFn: () => configRemoteApi.listKeys(),
  });
  const keys = data?.keys || [];

  const openEdit = (k: ConfigKeyRow) => {
    setEditing(k);
    setValue(k.value);
  };
  const save = async () => {
    if (!editing) return;
    try {
      await configRemoteApi.putKey(editing.key, value);
      message.success('已保存');
      setEditing(null);
      qc.invalidateQueries({ queryKey: ['config-remote-keys'] });
    } catch (e: any) {
      message.error(e.message || '保存失败');
    }
  };
  const remove = (k: ConfigKeyRow) => {
    Modal.confirm({
      title: `删除配置 ${k.key}?`,
      okType: 'danger',
      onOk: async () => {
        try {
          await configRemoteApi.deleteKey(k.key);
          message.success('已删除');
          qc.invalidateQueries({ queryKey: ['config-remote-keys'] });
        } catch (e: any) {
          message.error(e.message || '删除失败');
        }
      },
    });
  };

  return <>
    <ProTable<ConfigKeyRow> rowKey="key" loading={isLoading} search={false}
      style={{ marginTop: 16 }}
      dataSource={keys} headerTitle="Consul KV 配置（config/superops 前缀，写入即时生效于配置监听）"
      columns={[
        { title: 'Key', dataIndex: 'key', ellipsis: true },
        { title: '值', dataIndex: 'value', ellipsis: true, render: (_, r) => r.value || <Tag>空</Tag> },
        { title: '修改序号', dataIndex: 'modified_index', width: 110 },
        { title: '操作', width: 130, render: (_, r) => (
          <Space>
            <Button size="small" type="link" onClick={() => openEdit(r)}>编辑</Button>
            <Button size="small" type="link" danger onClick={() => remove(r)}>删除</Button>
          </Space>
        )},
      ]} />
    <Modal open={!!editing} title={editing?.key} onOk={save} onCancel={() => setEditing(null)}
      okText="保存" cancelText="取消" destroyOnClose>
      <Input.TextArea rows={8} value={value} onChange={(e) => setValue(e.target.value)} placeholder="配置值" />
    </Modal>
  </>;
}
