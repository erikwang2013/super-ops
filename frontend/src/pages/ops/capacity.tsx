import { StatisticCard } from '@ant-design/pro-components';
import { Card, Col, Row, Segmented } from 'antd';
import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { CapacityPoint, capacityApi } from '../../services/api';

interface Point { label: string; value: number; }

function TrendChart({ points, color }: { points: Point[]; color: string }) {
  const W = 900, H = 220, PAD = 48;
  if (points.length === 0) {
    return <div style={{ padding: 48, textAlign: 'center', color: '#999' }}>暂无数据（需 collector 运行并写入 ClickHouse）</div>;
  }
  const vals = points.map((p) => Number(p.value) || 0);
  const max = Math.max(...vals, 1);
  const n = points.length;
  const x = (i: number) => (n === 1 ? W / 2 : PAD + (i * (W - 2 * PAD)) / (n - 1));
  const y = (v: number) => H - PAD - (v / max) * (H - 2 * PAD);
  const step = Math.max(1, Math.floor(n / 12));
  return (
    <svg width="100%" height={H} viewBox={`0 0 ${W} ${H}`}>
      <line x1={PAD} y1={y(0)} x2={W - PAD} y2={y(0)} stroke="#e8e8e8" strokeDasharray="4 4" />
      <polyline points={points.map((p, i) => `${x(i)},${y(Number(p.value) || 0)}`).join(' ')}
        fill="none" stroke={color} strokeWidth="2" />
      {points.map((p, i) => (
        <g key={i}>
          <circle cx={x(i)} cy={y(Number(p.value) || 0)} r="3" fill={color} />
          {i % step === 0 &&
            <text x={x(i)} y={H - 12} textAnchor="middle" fontSize="11" fill="#999">{p.label}</text>}
        </g>
      ))}
      <text x={PAD} y={14} fontSize="12" fill="#999">{Math.round(max)}</text>
    </svg>
  );
}

export default function CapacityPage() {
  const [hours, setHours] = useState(24);
  const { data: summary } = useQuery({ queryKey: ['capacity-summary'], queryFn: () => capacityApi.summary() });
  const { data: trend, isLoading } = useQuery({
    queryKey: ['capacity-trend', hours],
    queryFn: () => capacityApi.trend(hours),
  });
  const points = trend?.points || [];
  const cpu = points.map((p: CapacityPoint) => ({ label: String(p.bucket || '').slice(11, 16), value: p.cpu_cores }));
  const mem = points.map((p: CapacityPoint) => ({ label: String(p.bucket || '').slice(11, 16), value: p.mem_gib }));
  const cost = points.map((p: CapacityPoint) => ({ label: String(p.bucket || '').slice(11, 16), value: p.cost_yuan_day }));
  const s = summary;
  return (
    <div>
      <Card title="容量与成本估算" extra={
        <Segmented value={hours} onChange={(v) => setHours(v as number)}
          options={[{ label: '24h', value: 24 }, { label: '7d', value: 168 }]} />
      }>
        <Row gutter={[16, 16]}>
          <Col span={6}><StatisticCard statistic={{ title: '节点数', value: s?.node_count || 0 }} /></Col>
          <Col span={6}><StatisticCard statistic={{ title: 'CPU 核数', value: s?.cpu_cores || 0, precision: 1 }} /></Col>
          <Col span={6}><StatisticCard statistic={{ title: '内存 GiB', value: s?.mem_gib || 0, precision: 1 }} /></Col>
          <Col span={6}><StatisticCard statistic={{ title: '估算成本（元/月）', value: s?.cost_yuan_month || 0, precision: 2 }} /></Col>
        </Row>
        <Card size="small" title={`CPU 核数趋势 · 近 ${hours}h`} loading={isLoading} style={{ marginTop: 12 }}>
          <TrendChart points={cpu} color="#1677ff" />
        </Card>
        <Card size="small" title={`内存 GiB 趋势 · 近 ${hours}h`} loading={isLoading} style={{ marginTop: 12 }}>
          <TrendChart points={mem} color="#52c41a" />
        </Card>
        <Card size="small" title={`估算成本（元/天）趋势 · 近 ${hours}h`} loading={isLoading} style={{ marginTop: 12 }}>
          <TrendChart points={cost} color="#fa8c16" />
        </Card>
      </Card>
    </div>
  );
}
