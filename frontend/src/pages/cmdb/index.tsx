import { ProTable } from '@ant-design/pro-components';
import { Button, Modal, Form, Input, Select, Popconfirm, Tag, Tabs, Empty, Space, Alert, message } from 'antd';
import { PlusOutlined, SyncOutlined } from '@ant-design/icons';
import { useState } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { cmdbApi, AssetRow, CmdbAssetInput, topologyApi, TopologyData, TopoNode } from '../../services/api';

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

const NODE_FILLS: Record<string, string> = {
  host: '#16a34a', switch: '#2563eb', router: '#9333ea', app: '#ea580c', db: '#d4a017', storage: '#0891b2',
};

const NODE_W = 140;
const NODE_H = 52;

function layout(nodes: TopoNode[], w: number, h: number) {
  const cx = w / 2, cy = h / 2;
  const r = Math.max(150, (nodes.length * 80) / (2 * Math.PI) + 60);
  const pos = new Map<string, { x: number; y: number }>();
  nodes.forEach((n, i) => {
    const a = (2 * Math.PI * i) / Math.max(1, nodes.length) - Math.PI / 2;
    pos.set(n.key, { x: cx + r * Math.cos(a), y: cy + r * Math.sin(a) });
  });
  return pos;
}

function TopologyView() {
  const qc = useQueryClient();
  const { data, isLoading } = useQuery({
    queryKey: ['cmdb-topology'],
    queryFn: () => topologyApi.get(),
  });
  const sync = useMutation({
    mutationFn: () => topologyApi.sync(),
    onSuccess: (r) => {
      message.success(`拓扑同步完成：${r.synced} 资产 / ${r.edges} 依赖`);
      qc.invalidateQueries({ queryKey: ['cmdb-topology'] });
    },
    onError: (e: Error) => message.error(e.message),
  });
  if (isLoading) return <div style={{ padding: 48, textAlign: 'center', color: '#94a3b8' }}>加载拓扑…</div>;
  const topo: TopologyData = data || { provider: '', nodes: [], edges: [] };
  const W = 920, H = 560;
  const pos = layout(topo.nodes, W, H);
  return <div style={{ paddingTop: 8 }}>
    <Space style={{ marginBottom: 8 }}>
      <Button icon={<SyncOutlined />} loading={sync.isPending} onClick={() => sync.mutate()}>同步拓扑</Button>
      <span style={{ color: '#64748b' }}>图数据库后端：{topo.provider || '未配置（gateway.yaml graph 段）'}</span>
    </Space>
    {topo.nodes.length === 0
      ? <Empty description="无拓扑数据，请先同步" />
      : <svg viewBox={`0 0 ${W} ${H}`} width="100%" height={H} style={{ border: '1px solid #e2e8f0', borderRadius: 8, background: '#f8fafc' }}>
          <defs>
            <marker id="arrow" markerWidth="8" markerHeight="8" refX="8" refY="4" orient="auto">
              <path d="M0,0 L8,4 L0,8 z" fill="#94a3b8" />
            </marker>
          </defs>
          {topo.edges.map((e, i) => {
            const s = pos.get(e.src), d = pos.get(e.dst);
            if (!s || !d) return null;
            const x1 = s.x, y1 = s.y, x2 = d.x, y2 = d.y;
            const mx = (x1 + x2) / 2, my = (y1 + y2) / 2 - 30;
            return <path key={`e${i}`} d={`M${x1},${y1} Q${mx},${my} ${x2},${y2}`}
              fill="none" stroke="#94a3b8" strokeWidth="1.5" markerEnd="url(#arrow)" />;
          })}
          {topo.nodes.map((n) => {
            const p = pos.get(n.key);
            if (!p) return null;
            const x = p.x - NODE_W / 2, y = p.y - NODE_H / 2;
            const fill = NODE_FILLS[n.asset_type] || '#64748b';
            const stroke = n.status === 'active' ? '#16a34a' : '#ef4444';
            const short = n.name.length > 12 ? `${n.name.slice(0, 11)}…` : n.name;
            return <g key={n.key}>
              <rect x={x} y={y} width={NODE_W} height={NODE_H} rx={10} fill="#ffffff"
                stroke={stroke} strokeWidth={2} />
              <rect x={x} y={y} width={6} height={NODE_H} rx={3} fill={fill} />
              <text x={x + 16} y={y + 24} fontSize={14} fontWeight={600} fill="#0f172a">{short}</text>
              <text x={x + 16} y={y + 42} fontSize={11} fill="#64748b">{n.asset_type} · {n.env || '—'}</text>
            </g>;
          })}
        </svg>}
  </div>;
}

function AssetsTab() {
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
          <Input.TextArea placeholder="JSON，可选（depends_on: 依赖资产名列表会生成拓扑边）" rows={3} />
        </Form.Item>
      </Form>
    </Modal>
  </>;
}

export default function CmdbPage() {
  return <>
    <Alert type="info" showIcon style={{ marginBottom: 12 }}
      message={'资产 labels 中配置 depends_on（JSON 数组，如 {"depends_on":["db-m"]}）后，拓扑页可将依赖关系渲染为图'} />
    <Tabs
      items={[
        { key: 'list', label: '资产列表', children: <AssetsTab /> },
        { key: 'topology', label: '拓扑图', children: <TopologyView /> },
      ]}
    />
  </>;
}
