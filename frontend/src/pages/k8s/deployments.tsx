import { ProTable } from '@ant-design/pro-components';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { Button, Form, InputNumber, Modal, Space, message } from 'antd';
import { k8sApi, Deployment } from '../../services/k8s';
import { ClusterPicker, useClusterPicker } from './cluster-select';

type Op = { kind: 'scale' | 'restart' | 'delete'; ns: string; name: string; replicas?: number };

export default function DeploymentsPage({ clusterId: fixed }: { clusterId?: string }) {
  const { defaultCluster } = useClusterPicker();
  const [picked, setPicked] = useState('');
  const clusterId = fixed ?? (picked || defaultCluster || 'default');
  const qc = useQueryClient();
  const { data, isLoading } = useQuery({ queryKey: ['deployments', clusterId], queryFn: () => k8sApi.listDeployments(clusterId) });

  const [scaleOpen, setScaleOpen] = useState(false);
  const [scaleTarget, setScaleTarget] = useState<Deployment | null>(null);
  const [form] = Form.useForm();

  const op = useMutation<unknown, Error, Op>({
    mutationFn: (cmd: Op) => {
      if (cmd.kind === 'scale') return k8sApi.scaleDeployment(clusterId, cmd.ns, cmd.name, cmd.replicas!);
      if (cmd.kind === 'restart') return k8sApi.restartDeployment(clusterId, cmd.ns, cmd.name);
      return k8sApi.deleteDeployment(clusterId, cmd.ns, cmd.name);
    },
    onSuccess: (_d, cmd) => {
      qc.invalidateQueries({ queryKey: ['deployments', clusterId] });
      message.success({ scale: '扩容成功', restart: '重启成功', delete: '删除成功' }[cmd.kind]);
    },
    onError: (e: Error) => message.error(e.message),
  });

  const openScale = (d: Deployment) => {
    setScaleTarget(d);
    form.setFieldsValue({ replicas: d.replicas });
    setScaleOpen(true);
  };

  const confirmRestart = (d: Deployment) => {
    Modal.confirm({
      title: `重启 ${d.namespace}/${d.name}`,
      content: `将重启 Deployment ${d.namespace}/${d.name}（滚动更新）。是否继续？`,
      okText: '重启',
      cancelText: '取消',
      onOk: () => op.mutate({ kind: 'restart', ns: d.namespace, name: d.name }),
    });
  };

  const confirmDelete = (d: Deployment) => {
    Modal.confirm({
      title: `删除 ${d.namespace}/${d.name}`,
      content: `将删除 Deployment ${d.namespace}/${d.name}，此操作不可恢复（启用审批时需先提交删除审批单）。是否继续？`,
      okText: '删除',
      cancelText: '取消',
      okButtonProps: { danger: true },
      onOk: () => op.mutate({ kind: 'delete', ns: d.namespace, name: d.name }),
    });
  };

  return <>
    {!fixed && <ClusterPicker value={picked} onChange={setPicked} />}
    <ProTable<Deployment> columns={[
      { title: '名称', dataIndex: 'name' }, { title: '命名空间', dataIndex: 'namespace' },
      { title: '副本', dataIndex: 'replicas' }, { title: '就绪', dataIndex: 'ready_replicas' },
      { title: '运行时间', dataIndex: 'age' },
      { title: '操作', valueType: 'option', render: (_, d) => (
        <Space size={4}>
          <Button size="small" type="link" onClick={() => openScale(d)}>扩容</Button>
          <Button size="small" type="link" onClick={() => confirmRestart(d)}>重启</Button>
          <Button size="small" type="link" danger onClick={() => confirmDelete(d)}>删除</Button>
        </Space>
      ) },
    ]} dataSource={data?.deployments || []} loading={isLoading} rowKey="name" search={false} headerTitle={`Deployments（集群 ${clusterId}）`} />
    <Modal title={scaleTarget ? `扩容/缩容 ${scaleTarget.namespace}/${scaleTarget.name}` : '扩容/缩容'} open={scaleOpen}
      onCancel={() => setScaleOpen(false)} onOk={() => form.submit()} confirmLoading={op.isPending}>
      <Form form={form} layout="vertical" onFinish={(v: { replicas: number }) => {
        if (scaleTarget) op.mutate({ kind: 'scale', ns: scaleTarget.namespace, name: scaleTarget.name, replicas: v.replicas });
        setScaleOpen(false);
      }}>
        <Form.Item name="replicas" label="副本数（0-1000）" rules={[{ required: true, message: '请输入副本数' }]}>
          <InputNumber min={0} max={1000} style={{ width: '100%' }} />
        </Form.Item>
      </Form>
    </Modal>
  </>;
}
