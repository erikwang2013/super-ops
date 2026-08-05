import { ProList } from '@ant-design/pro-components';
import { Button, Tag, Modal, Form, Input, message } from 'antd';
import { PlusOutlined } from '@ant-design/icons';
import { useNavigate } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { k8sApi, Cluster } from '../../services/k8s';
import { useState } from 'react';

export default function ClustersPage() {
  const nav = useNavigate(); const qc = useQueryClient();
  const [open, setOpen] = useState(false); const [form] = Form.useForm();
  const { data, isLoading } = useQuery({ queryKey: ['clusters'], queryFn: () => k8sApi.listClusters() });
  const add = useMutation({
    mutationFn: (v: { name: string; kubeconfig: string }) => k8sApi.addCluster(v.name, v.kubeconfig),
    onSuccess: () => { qc.invalidateQueries({ queryKey: ['clusters'] }); setOpen(false); form.resetFields(); message.success('添加成功'); },
    onError: (e: Error) => message.error(e.message),
  });

  return <>
    <ProList<Cluster> rowKey="id" loading={isLoading} dataSource={data?.clusters || []}
      metas={{ title: { dataIndex: 'name' }, description: { dataIndex: 'version' },
        content: { render: (_, r) => (<div style={{ display:'flex', gap:16 }}><Tag>{r.status}</Tag><span>节点:{r.node_count}</span><span>Pods:{r.pod_count}</span></div>) } }}
      headerTitle="集群列表" toolBarRender={() => [<Button key="add" type="primary" icon={<PlusOutlined />} onClick={() => setOpen(true)}>添加集群</Button>]}
      onRow={(r) => ({ onClick: () => nav(`/k8s/clusters/${r.id}`), style: { cursor:'pointer' } })}
      locale={{ emptyText: '暂无集群' }} />
    <Modal title="添加 K8s 集群" open={open} onCancel={() => setOpen(false)} onOk={() => form.submit()} confirmLoading={add.isPending}>
      <Form form={form} onFinish={(v) => add.mutate(v)} layout="vertical">
        <Form.Item name="name" label="名称" rules={[{ required: true }]}><Input placeholder="prod-cluster-1" /></Form.Item>
        <Form.Item name="kubeconfig" label="Kubeconfig" rules={[{ required: true }]}><Input.TextArea rows={8} placeholder="粘贴 kubeconfig 内容" /></Form.Item>
      </Form>
    </Modal>
  </>;
}
