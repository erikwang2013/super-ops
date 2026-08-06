import { StatisticCard } from '@ant-design/pro-components';
import { Card, Col, Row, Select } from 'antd';
import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api } from '../../services/api';

interface MetricPreset { label: string; measurement: string; series: string; field: string; }
// 指标选择器：与后端实际写入的 measurement/field 对齐（collector ch.rs/housekeeping.rs）
const PRESETS: MetricPreset[] = [
  { label: 'CPU 核数', measurement: 'capacity_snapshot', series: 'cluster', field: 'cpu_cores' },
  { label: '内存 GiB', measurement: 'capacity_snapshot', series: 'cluster', field: 'mem_gib' },
  { label: '节点总数', measurement: 'capacity_snapshot', series: 'cluster', field: 'node_count' },
  { label: 'Pod 运行数', measurement: 'resource_snapshot', series: 'type', field: 'pod_running' },
  { label: '节点就绪', measurement: 'resource_snapshot', series: 'node', field: 'ready' },
  { label: 'Pod 重启次数', measurement: 'resource_snapshot', series: 'pod', field: 'restarts' },
];

interface MetricRow { series: string; value: number; ts: number; }

function TrendChart({ rows }: { rows: MetricRow[] }) {
  const W = 640, H = 200, PAD = 40;
  if (rows.length === 0) {
    return <div style={{ padding: 48, textAlign: 'center', color: '#999' }}>暂无数据</div>;
  }
  const vals = rows.map((r) => Number(r.value) || 0);
  const max = Math.max(...vals, 1);
  const min = Math.min(...vals, 0);
  const span = max - min || 1;
  const n = rows.length;
  const x = (i: number) => (n === 1 ? W / 2 : PAD + (i * (W - 2 * PAD)) / (n - 1));
  const y = (v: number) => H - PAD - ((v - min) / span) * (H - 2 * PAD);
  return (
    <svg width="100%" height={H} viewBox={`0 0 ${W} ${H}`}>
      <line x1={PAD} y1={y(0)} x2={W - PAD} y2={y(0)} stroke="#e8e8e8" strokeDasharray="4 4" />
      <polyline points={rows.map((r, i) => `${x(i)},${y(Number(r.value) || 0)}`).join(' ')}
        fill="none" stroke="#1677ff" strokeWidth="2" />
      {rows.map((r, i) => (
        <g key={r.series}>
          <circle cx={x(i)} cy={y(Number(r.value) || 0)} r="3.5" fill="#1677ff" />
          <text x={x(i)} y={y(Number(r.value) || 0) - 8} textAnchor="middle" fontSize="11">{Number(r.value) || 0}</text>
          <text x={x(i)} y={H - 10} textAnchor="middle" fontSize="11" fill="#999">{r.series}</text>
        </g>
      ))}
    </svg>
  );
}

export default function MetricsPage() {
  const [idx, setIdx] = useState(0);
  const preset = PRESETS[idx];
  const { data, isLoading } = useQuery({
    queryKey: ['metrics', preset],
    queryFn: () => api.get<MetricRow[]>(
      `/v1/metrics/query?measurement=${preset.measurement}&series=${preset.series}&field=${preset.field}&window_secs=3600`),
  });
  const rows = data || [];
  const latest = Math.round(rows.reduce((s, r) => s + (Number(r.value) || 0), 0) * 100) / 100;
  // 行序不保证按时间排列，取最新时间戳展示
  const at = rows.length ? new Date(Math.max(...rows.map((r) => r.ts)) * 1000).toLocaleString() : '-';
  return (
    <div>
      <Card title="指标看板">
        <Select value={idx} style={{ width: 220 }} onChange={setIdx}
          options={PRESETS.map((p, i) => ({ value: i, label: p.label }))} />
      </Card>
      <Row gutter={[16, 16]} style={{ marginTop: 16 }}>
        <Col span={8}>
          <StatisticCard statistic={{ title: '最近值', value: latest, description: `${preset.field} · 近 1h` }} />
        </Col>
        <Col span={8}><StatisticCard statistic={{ title: 'Series 数', value: rows.length, description: at }} /></Col>
        <Col span={8}><StatisticCard statistic={{ title: '窗口', value: '1h', description: 'window_secs=3600' }} /></Col>
      </Row>
      <Card title={`趋势 · ${preset.label}`} loading={isLoading} style={{ marginTop: 16 }}>
        <TrendChart rows={rows} />
      </Card>
    </div>
  );
}
