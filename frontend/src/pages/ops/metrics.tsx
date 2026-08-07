import { StatisticCard } from '@ant-design/pro-components';
import { Button, Card, Col, Empty, Row, Segmented, Select, Space, message } from 'antd';
import { DownloadOutlined } from '@ant-design/icons';
import { useEffect, useRef, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import * as echarts from 'echarts/core';
import { BarChart, LineChart } from 'echarts/charts';
import { GridComponent, LegendComponent, TooltipComponent } from 'echarts/components';
import { CanvasRenderer } from 'echarts/renderers';
import { api } from '../../services/api';

echarts.use([BarChart, LineChart, GridComponent, LegendComponent, TooltipComponent, CanvasRenderer]);

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

const WINDOWS = [
  { label: '1h', secs: 3600, bucket: 5 },
  { label: '6h', secs: 21600, bucket: 15 },
  { label: '24h', secs: 86400, bucket: 60 },
  { label: '7d', secs: 604800, bucket: 360 },
] as const;

interface MetricRow { bucket: string; series: string; value: number; }

export default function MetricsPage() {
  const [idx, setIdx] = useState(0);
  const [win, setWin] = useState(0);
  const [kind, setKind] = useState<'line' | 'bar'>('line');
  const preset = PRESETS[idx];
  const w = WINDOWS[win];
  const { data, isLoading } = useQuery({
    queryKey: ['metrics-trend', preset, w],
    queryFn: () => api.get<MetricRow[]>(
      `/metrics/trend?measurement=${preset.measurement}&series=${preset.series}&field=${preset.field}&window_secs=${w.secs}&bucket_minutes=${w.bucket}`),
  });
  const rows = data || [];

  const chartRef = useRef<HTMLDivElement | null>(null);
  const chart = useRef<ReturnType<typeof echarts.init> | null>(null);
  const hasRows = rows.length > 0;

  useEffect(() => {
    if (!chartRef.current || !hasRows) return;
    chart.current = echarts.init(chartRef.current);
    const onResize = () => chart.current?.resize();
    window.addEventListener('resize', onResize);
    return () => {
      window.removeEventListener('resize', onResize);
      chart.current?.dispose();
      chart.current = null;
    };
  }, [hasRows]);

  useEffect(() => {
    if (!chart.current) return;
    const buckets = Array.from(new Set(rows.map((r) => r.bucket))).sort();
    const names = Array.from(new Set(rows.map((r) => r.series))).sort();
    const series = names.map((s) => ({
      name: s,
      type: kind,
      smooth: kind === 'line',
      data: buckets.map((b) => rows.find((r) => r.series === s && r.bucket === b)?.value ?? null),
    }));
    chart.current.setOption({
      tooltip: { trigger: 'axis' },
      legend: { type: 'scroll', top: 0 },
      grid: { left: 48, right: 24, top: 32, bottom: 48 },
      xAxis: { type: 'category', data: buckets, axisLabel: { rotate: buckets.length > 12 ? 30 : 0 } },
      yAxis: { type: 'value' },
      series,
    }, true);
  }, [rows, kind]);

  const exportCsv = () => {
    if (!rows.length) { message.warning('无数据可导出'); return; }
    const header = 'bucket,series,value\n';
    const body = rows.map((r) => `${r.bucket},${r.series},${r.value}`).join('\n');
    const blob = new Blob(['﻿' + header + body], { type: 'text/csv;charset=utf-8' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `metrics-${preset.field}-${w.secs}s.csv`;
    a.click();
    URL.revokeObjectURL(url);
    message.success('CSV 已导出');
  };

  const latestBySeries = new Map<string, number>();
  for (const r of rows) latestBySeries.set(r.series, r.value); // 行按 bucket 升序，后者即各序列最新值
  const latest = Math.round(Array.from(latestBySeries.values()).reduce((s, v) => s + v, 0) * 100) / 100;

  return (
    <div>
      <Card title="指标看板">
        <Space wrap>
          <Select value={idx} style={{ width: 220 }} onChange={setIdx}
            options={PRESETS.map((p, i) => ({ value: i, label: p.label }))} />
          <Segmented value={win} onChange={(v) => setWin(v as number)}
            options={WINDOWS.map((x, i) => ({ label: x.label, value: i }))} />
          <Segmented value={kind} onChange={(v) => setKind(v as 'line' | 'bar')}
            options={[{ label: '折线', value: 'line' }, { label: '柱状', value: 'bar' }]} />
          <Button icon={<DownloadOutlined />} onClick={exportCsv}>导出 CSV</Button>
        </Space>
      </Card>
      <Row gutter={[16, 16]} style={{ marginTop: 16 }}>
        <Col span={8}>
          <StatisticCard statistic={{ title: '最近值（Σ各序列）', value: latest, description: `${preset.field} · ${w.label}` }} />
        </Col>
        <Col span={8}><StatisticCard statistic={{ title: 'Series 数', value: latestBySeries.size }} /></Col>
        <Col span={8}><StatisticCard statistic={{ title: '窗口', value: w.label, description: `${w.bucket}min 桶` }} /></Col>
      </Row>
      <Card title={`趋势 · ${preset.label}（按 ${preset.series} 分组）`} loading={isLoading} style={{ marginTop: 16 }}>
        {hasRows
          ? <div ref={chartRef} style={{ width: '100%', height: 360 }} />
          : <Empty description="暂无数据（需 collector 运行并写入 ClickHouse）" style={{ padding: 48 }} />}
      </Card>
    </div>
  );
}
